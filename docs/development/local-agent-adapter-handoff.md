# 本机 Agent 适配层总装交接

## 分支与基线

- 专属分支：`feat/local-agent-adapter`
- 起点：`e00acb71cf02dcc04e78f13ec76b63b6413a93d0`（`feat/macos-director-console`）
- 独立 worktree：`/Users/chenhuajin/项目/自己的应用/infinite-canvas-worktrees/local-agent-adapter`
- 本分支不合并其他功能分支，也不改写主检出。

## 已完成

- 独立 Rust crate 提供 loopback HTTP Bridge、机器可读 capabilities、安装凭据、撤销轮换、结构化错误和 CLI。
- Tauri 启动/停止 Bridge，并将现有 DesktopRuntime 的探测、零付费固定测试片、任务状态和取消适配到白名单 trait。
- 桌面 WebView 通过 Tauri IPC 使用现有 SQLite `canvas_projects`；
  外部 Agent 经 `CanvasOperationAdapter` 使用同一表。
- Agent 操作带 project/request/base revision/actor，支持 dry-run、compare-and-swap、持久幂等和人工锁定优先。
- Agent 可用同一公共协议创建工程，并可把应用私有固定 inbox 中、摘要匹配的 MP4
  摄入为同一 `CanvasProject` 内的 video 节点和 canvas task；Rust executor 的
  ffprobe/转码结果由同一 system 批次直接回填 task 终态和节点稳定引用、尺寸、
  时长、摘要，不创建第二张画布数据库或第二套 revision，也不依赖 UI 已打开。
- CLI 作为 Tauri `externalBin` 随 `.app` 打包；不接受 token 参数，仅从私有凭据文件读取。
- React 画布 UI 未重做，只改了桌面持久化接线和首次本地项目合并。

## 总装结果

本 handoff 的替换任务已在 `feat/human-agent-canvas-integration` 完成：

```text
desktop/src-tauri/src/agent_bridge.rs
  DesktopAgentBridge::start
    CanonicalCanvasAdapter::open(...)
```

端口预检仍为正式 `3100/3101/3102`，Bridge 在 Go 初始化后启动，退出时先停
Bridge 再停 DesktopRuntime。桌面 Store 合并改为 revision 优先、同 revision
再比 `updatedAt`。完整决策和真实验收见
[人与本机 Agent 共用无限画布总装 Handoff](human-agent-canvas-integration-handoff.md)。

必须保持的外部协议：

- `GET /v1/capabilities`
- `GET /v1/projects`
- `GET /v1/projects/{project_id}`
- `POST /v1/projects`
- `POST /v1/canvas/operations/dry-run`
- `POST /v1/canvas/operations/apply`
- `GET /v1/media/inbox`
- `POST /v1/media/video-ingests`
- `GET /v1/runtime`
- `POST /v1/tasks/test-clips`
- `GET /v1/tasks/{task_id}`
- `POST /v1/tasks/{task_id}/cancel`
- `POST /v1/credentials/revoke`
- CLI 命令名、JSON envelope 和退出码

新增 CLI：

```text
infinite-canvas projects create --file request.json
infinite-canvas media inbox
infinite-canvas media video ingest --file request.json
```

视频请求没有路径或 URL 字段，只接收 inbox basename、小写 SHA-256、工程/节点/
request ID、title、position 和 size。桌面端拒绝目录穿越、符号链接、非 MP4、空文件、
超上限和摘要不匹配；输出固定写入应用私有 verified 根并再次验证 SHA/ffprobe。

旧 adapter 的直接 reducer/SQL、SHA revision 和独立 request journal 已删除。
Bridge、CredentialStore、CLI、AgentRuntime trait、外部 JSON 和退出码保持兼容。

## 验证命令

```bash
cd integrations/local-agent-adapter-rust
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features

cd ../../desktop
bun install --frozen-lockfile
bun run stage

cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features

cd ../../../
go test ./...

cd web
bun install --frozen-lockfile
bun run build
```

`bunx tsc --noEmit` 在指定基线已有若干与本分支无关的错误；
报错文件均未被本分支修改。生产 `next build` 已通过，但总装若修复全仓
TypeScript 基线，可再把 `tsc --noEmit` 设为硬门禁。

## 总装后的剩余发行事项

- 标准技术 `.app` 已确认 CLI 位于 `Contents/MacOS/infinite-canvas` 且为 arm64；
  Developer ID 签名、公证后的包需要再次复核 CLI 和软链方案。
- 真实共享状态、锁、stale revision、撤销、重启与 ZIP 往返已在隔离 bundle ID
  下通过；用户也已在备份后完成第一阶段正式 SQLite/Bridge 迁移验收。
- 受控工程创建与单个 MP4 摄入已在隔离 App 跑通，并发现/修复 WebView 瞬时伪
  revision、重复 runtime task 和幂等响应 revision 三处问题；最终 ZIP 回灌与
  真实 UI 媒体批次撤销也已通过，完整证据见总装 handoff。
- 2026-08-31 正式冒烟追加发现并修复 headless 完工只写 task、UI 物化补写 bytes
  制造伪 revision，以及 H.264 被重编码为 MPEG-4 Part 2 三处问题。修复后的
  255441-byte H.264/AAC 输入走 stream copy，输出 254832 bytes；CLI-only 完工为
  revision 4，打开并播放后仍为 revision 4。
- 没有调用付费模型，没有修改 Eagle/达芬奇或正式用户项目，没有提交真实凭据。
