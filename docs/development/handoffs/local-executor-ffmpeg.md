# Rust 本地执行核心与 FFmpeg 交接

## 交付边界

本分支只新增独立 crate `integrations/local-executor-rust/`，没有接入 Tauri，也没有修改 Web、Go、桌面壳或 `docs/development/macos-director-acceptance-matrix.md`。总装任务可在确认 Tauri 生命周期和文件选择器接口后，把该 crate 作为库接入。

## 已实现接口

入口从 `local_executor` crate 导入：

- `Toolchain::discover(ToolDiscoveryConfig)`：只在宿主注册的可信目录中寻找文件名精确为 `ffmpeg`、`ffprobe` 的可执行文件，并分别执行 `-version` 校验首行。
- `AllowedRoot::new(RootId, absolute_path)`：由宿主注册已经过用户选择或属于项目/测试范围的绝对目录。任务请求不能注册目录。
- `Executor::new(ExecutorConfig)`：创建单工作线程执行器并加载 JSON 状态日志。
- `Executor::submit(TaskRequest)`：提交强类型白名单动作，返回 `Accepted(task_id)` 或 `Duplicate(existing_task_id)`。
- `Executor::task(task_id)` / `wait(task_id, timeout)`：读取任务快照。
- `Executor::cancel(task_id)`：取消排队或运行中的任务。
- `Executor::events()`：读取不含路径、参数、stdout、stderr 或幂等原文的结构化内存事件。

任务状态固定为：

```text
queued -> running -> succeeded | failed | cancelled
queued -> cancelled
```

当前白名单动作固定为：

1. `GenerateTestClip`：FFmpeg `lavfi` 测试图和测试音生成短 MP4。
2. `TranscodeToMp4`：本地视频转 MPEG-4/AAC MP4。
3. `VerifyMedia`：ffprobe 结构探测、`ffmpeg -xerror` 全流解码和 SHA-256。

请求没有命令、程序路径或自由参数字段。未知动作、未知字段、shell 元字符、绝对路径、`..`、不存在的根 ID、符号链接越界和未注册目录都会被拒绝。所有外部进程通过 `std::process::Command` 的参数数组直接启动，不经过 shell。

## 幂等、输出与恢复语义

- `TaskId` 是 UUID，首次提交后写入状态文件。同一幂等键和同一请求指纹在进程内或重启后都返回原任务 ID；同一键对应不同请求返回 `IdempotencyConflict`。最终插入点有锁内复核，已测试 8 线程并发只能创建一个任务。
- 状态文件只保存幂等键 SHA-256、请求指纹、任务 ID、状态、相对作用域路径、结果和结构化错误；不保存幂等原文、绝对项目路径、进程参数或工具输出。
- `Reject` 在已有输出时拒绝任务；`UniqueSuffix` 依次选择 `name-1.mp4` 等新名字。
- FFmpeg 始终写同目录唯一 `.part.mp4`。ffprobe、完整解码和 SHA-256 全部成功后，通过 `hard_link` 的“目标必须不存在”语义发布，再删除临时名字；不会调用覆盖写。
- 排队或运行中的任务在异常重启后不会自动重放，而会恢复为 `failed / interrupted_by_restart`。已经终态的任务和幂等索引会恢复。
- 正常取消会杀掉当前直接子进程并清理本次部分输出。宿主正常析构执行器时也会取消未完成任务。

明确的恢复边界：操作系统或进程被强制杀死时，可能留下隐藏的、带随机 UUID 的 `.part.mp4`，当前实现不自动扫描或删除它；最终目标文件仍不会被覆盖。状态写入采用同目录临时文件、`sync_all`、原子重命名。执行器是单工作线程，尚未实现跨进程锁，因此同一个状态目录不能同时启动两个执行器实例。

## 给 Tauri 总装任务的接线建议

1. 在 Tauri 启动阶段创建唯一的 `Arc<Executor>`，状态目录由应用内部决定，不要让 IPC 请求提供。
2. 用户通过原生文件/目录选择器选择项目目录后，由 Rust 宿主创建 `RootId -> AllowedRoot`；IPC 只接收 `RootId + relative path`。
3. 把应用内捆绑工具目录或受信任的 Homebrew 目录传入 `ToolDiscoveryConfig`。不要允许前端动态提交任意可执行文件路径。
4. IPC 命令只做 `submit/status/cancel/events` 的薄适配。`TaskRequest`、`TaskSnapshot` 和错误类型已经实现 Serde。
5. 把 `ExecutorError` 映射为 IPC 立即失败；已接受任务的运行错误从 `TaskSnapshot.error` 读取。不要把 FFmpeg stderr 原文回传前端或写日志。
6. 关闭应用前先释放执行器，使运行任务收到取消。不要同时为同一状态目录创建第二个执行器。
7. 总装时补 Tauri capability、IPC 入参和文件选择器端到端测试；本分支没有假装这些已经完成。

## 可复现验收

Rust 工具链需要加入 PATH：

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

格式、单元测试、真实 FFmpeg 集成测试和静态检查：

```sh
cargo fmt --manifest-path integrations/local-executor-rust/Cargo.toml -- --check
cargo test --manifest-path integrations/local-executor-rust/Cargo.toml
cargo test --manifest-path integrations/local-executor-rust/Cargo.toml \
  --test ffmpeg_integration -- --ignored --nocapture
cargo clippy --manifest-path integrations/local-executor-rust/Cargo.toml \
  --all-targets -- -D warnings
```

独立零付费样例：

```sh
executor_sample_dir="$(mktemp -d)"
cargo run --quiet --manifest-path integrations/local-executor-rust/Cargo.toml \
  --bin local-executor-demo -- sample "$executor_sample_dir"
ffprobe -v error \
  -show_entries format=duration,size:stream=index,codec_type,codec_name,width,height,sample_rate,channels \
  -of json "$executor_sample_dir/deterministic-sample.mp4"
ffmpeg -hide_banner -nostdin -v error -xerror \
  -i "$executor_sample_dir/deterministic-sample.mp4" -map 0 -f null -
shasum -a 256 "$executor_sample_dir/deterministic-sample.mp4"
```

本分支现场结果：

- Rust `1.92.0`，Cargo `1.92.0`。
- FFmpeg `8.1`，ffprobe `8.1`，版本探测通过。
- 单元测试 `11 passed / 0 failed`，包含验证失败后部分输出清理。
- 真实集成测试 `1 passed / 0 failed`；完成生成、ffprobe、完整 `-xerror` 解码、转码、输出冲突拒绝、唯一后缀和终态重启恢复。
- Clippy `--all-targets -D warnings` 通过。
- 手工样例：1.000 秒，138,603 bytes；视频 `mpeg4`、320x180；音频 `aac`、48 kHz、单声道；两个全新临时目录独立生成的 SHA-256 均为 `d3bf7ba437acab289ed29638f3e481004c5828af84525c4e1d1c76d47fe1dddd`；ffprobe 与完整解码退出码均为 0。
- 没有调用付费服务，没有读取或覆盖用户真实素材；样例只写入系统临时目录。

## 事实、推断、未知

### 事实

- 本 crate 没有 shell 执行入口，也没有自由命令/参数字段。
- 路径以宿主注册根目录和相对路径解析，现有测试覆盖路径穿越、shell 元字符和符号链接逃逸。
- 取消、总超时、子进程退出码、输出冲突、并发重复提交、重启中断和日志脱敏都有自动测试。
- 真实 FFmpeg 样例和转码产物已经过 ffprobe 与完整解码。

### 推断

- 该 API 适合作为 Tauri managed state，但最终 IPC 形状仍应由总装任务按现有壳结构决定。
- 同目录 hard-link 发布适合目标 macOS 文件系统；若未来允许不支持 hard link 的卷，需要另行设计同等“绝不覆盖”的发布原语。

### 未知

- Tauri 最终应用支持目录、项目根目录注册/撤销生命周期和前端状态展示尚未实现。
- FFmpeg 是否随应用捆绑、签名和公证策略尚未确定；当前验收使用 `/opt/homebrew/bin`。
- 强制崩溃后的隐藏临时文件清理策略尚未实现。
- 未验证两个操作系统进程错误地共用同一状态目录；接线必须保证单实例。
- 未做正式用户素材、长视频、DMG 或分发环境验收。
