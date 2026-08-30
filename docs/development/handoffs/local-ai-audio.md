# 本地 AI 声音 Provider 交接

## 交付范围

本分支新增 `integrations/local-ai-audio-rust/`，提供 IndexTTS-2.5 与 VoxCPM2 的本机发现、状态报告、固定 smoke 生成和音频完整性验收。它没有接入 Tauri、Web 或 Go，也没有调用云端/付费接口。

验收产物固定写入 crate 内的 `.acceptance/<unique-run>/sample.wav`，该目录与 Rust `target/` 均已加入仓库忽略规则。每次运行使用原子创建的唯一目录并拒绝覆盖；失败时整次运行目录会被清理。提交中不含音频大文件、用户文案、个人参考声音、凭据或机器绝对安装路径。

## Provider 与状态协议

协议版本为 `1`，Provider ID 为：

- `index_tts_25`
- `vox_cpm_2`

`ProviderReport.status` 的语义：

| 状态 | 含义 |
| --- | --- |
| `not_found` | 配置的发现根目录内未发现安装 |
| `discovered` | 安装、模型和运行时已识别，但服务探测被关闭 |
| `ready` | 固定回环端口返回 2xx 且页面内容匹配 Provider 身份 |
| `not_running` | 安装、模型、运行时完整，但固定回环端口未监听 |
| `model_missing` | 缺少该版本的必要模型文件 |
| `incompatible` | 隔离 Python 运行时缺失或版本不在支持范围 |
| `error` | 端口被其他服务占用、响应身份不匹配或探测异常 |

`ready` 只表示“本地服务可响应”，不表示生成已通过。`ProviderReport.end_to_end` 在普通探测中始终为 `not_run`；只有一次真实生成成功、`ffprobe` 识别成功且 `ffmpeg -xerror` 全量解码成功后，`SmokeReport.end_to_end` 才会是 `passed`。这样不会把进程存在、网页可开或 HTTP 200 冒充端到端通过。

能力模型当前声明：

- 两者都支持语音合成、参考音频和 WAV 输出。
- 只有 VoxCPM2 声明 `voice_design=true`。
- IndexTTS-2.5 的固定验收使用上游仓库自带 `examples/voice_01.wav`，不使用个人声音。
- VoxCPM2 的固定验收使用 voice-design 模式，不提供任何参考音频。

每条探测证据带 `fact`、`inference` 或 `unknown` 类型。当前实现只自动写入可直接复核的 `fact`；推断与未知留给上层展示或本交接文档说明。

## Rust 接口

核心入口：

```rust
use local_ai_audio::{
    ApprovedInstallation, DiscoveryConfig, ProviderId, probe_all, run_smoke, verify_audio,
};

let report = probe_all(&DiscoveryConfig::from_env()?);
let audio = verify_audio(path)?;
let approved = ApprovedInstallation::new(ProviderId::IndexTts25, user_approved_path)?;
let smoke = run_smoke(&approved)?;
```

为后续 Tauri IPC 预留的 `IpcRequest` 是封闭枚举，只允许：

- `Discover { roots }`
- `VerifyAudio { path }`
- `SmokeTest { provider, installation }`

`installation` 必须来自用户明确批准的桌面目录选择。接口中不存在 shell 命令、可执行文件、任意 URL、Host、端口或凭据字段。服务探测只访问 Provider 固定的 `127.0.0.1:7860` 与 `127.0.0.1:8808`。

发现路径按以下顺序配置，不把机器路径写死在代码中：

1. `LOCAL_AI_AUDIO_INDEXTTS_HOME` / `LOCAL_AI_AUDIO_VOXCPM_HOME`：显式安装目录，仍会校验 Provider 标记。
2. `LOCAL_AI_AUDIO_DISCOVERY_ROOTS`：使用平台路径分隔符提供一个或多个发现根目录。
3. 无显式配置时，以当前用户 Home 为发现根目录，限制深度和最多扫描目录数，并跳过隐藏目录、`Library`、`node_modules`、`target`、`dist`、`build` 与 `data`。

命令行验收入口：

```bash
cargo run -- probe --root /path/to/discovery-root
cargo run -- verify-audio /path/to/audio.wav
cargo run -- smoke index_tts_25 --install /user/approved/IndexTTS
cargo run -- smoke vox_cpm_2 --install /user/approved/VoxCPM
```

两个 smoke 都只使用固定短句“本地语音测试完成。”，强制 Hugging Face/Transformers 离线，只执行一次且不自动重试。输出只进入 `.acceptance/`。

### 执行安全边界

- Discovery 永远只返回报告，不返回 `ApprovedInstallation`，也不会执行发现结果。CLI 缺少 `--install` 会直接退出；即使扫描到 marker 完整目录也不会自动选择排序首项。
- `ApprovedInstallation::new` 仅接收用户明确批准的目录；创建时 canonicalize 并核对 Provider、模型、Python 和固定 smoke 入口，真正执行前再次 canonicalize、复核标记并确认目标没有变化。
- 模型子进程全部 `env_clear`，只设置隔离的临时 `HOME/TMPDIR`、固定系统 `PATH`、UTF-8、Python 隔离和离线开关；不继承 token、代理、云凭据或其他父进程环境。成功后临时 runtime 目录会删除。
- 模型 smoke 超时 300 秒，stdout/stderr 各限 256 KiB；`ffprobe` 超时 15 秒，`ffmpeg -xerror` 超时 120 秒，媒体输出各限 128 KiB。超时或输出超限都会 kill 并 wait 回收直接子进程。
- `ffprobe`/`ffmpeg` 只从代码内部的固定绝对候选解析并 canonicalize，不读取前端传入的可执行路径，也不直接依赖可劫持的父进程 `PATH`。
- 返回的模型日志会替换安装、crate 和输出绝对路径，并丢弃包含授权、API Key、密码、token 或代理字段的整行。
- 每次 smoke 原子创建唯一 run 目录，输出必须原先不存在；子进程失败、超时、输出超限或音频验证失败都会由清理守卫移除整个 run 目录，不留下零字节或未验证的“成功”文件。

## 本机现场证据

普通探测结果：

| Provider | 安装 | 模型标记 | Python | 服务 | 普通探测 E2E |
| --- | --- | --- | --- | --- | --- |
| IndexTTS-2.5 | 已发现 | 16/16 | 3.11.13，兼容 | `127.0.0.1:7860` connection refused，`not_running` | `not_run` |
| VoxCPM2 | 已发现 | 5/5 | 3.11.13，兼容 | `127.0.0.1:8808` connection refused，`not_running` | `not_run` |

IndexTTS-2.5 自带的 `indextts2 check` 面向旧版 IndexTTS2 资源清单，会因不存在 `bpe.model` 报错；2.5 WebUI 自己要求的是 `multilingual_zh_ja_yue_char_del.tiktoken`。本实现按 2.5 运行代码的 16 项资源清单探测，并已用真实模型加载与生成结果证明当前资源可用，没有为了迎合旧检查器下载或改写模型。

真实 smoke 结果：

| Provider | 模式与参数边界 | 音频证据 | 非静音证据 | E2E |
| --- | --- | --- | --- | --- |
| IndexTTS-2.5 | MPS；ZH；上游 `voice_01.wav`；禁用情感文本；seed 42；duration 1.0 | 2.229116 秒，PCM s16le，22050 Hz，单声道；SHA-256 `010ff48713a84b18db54db38a732c9ab5fe61246c5acdd152e73bb93862b0559` | mean -20.1 dB，max -3.8 dB | `ffprobe=true`，`ffmpeg -xerror` 全解码通过 |
| VoxCPM2 | voice design；MPS FP32；control“清晰自然的普通话声音”；CFG 2.0；4 steps；seed 42；禁用 denoiser；local-files-only | 2.56 秒，PCM s16le，48000 Hz，单声道；SHA-256 `eabb115d87ac173f8fa08deeb1eb23fe3e2a64d28f14a71e1e216290b04a82a2` | mean -30.6 dB，max -8.0 dB | `ffprobe=true`，`ffmpeg -xerror` 全解码通过 |

安全边界修复后，使用显式批准路径、`env_clear`、固定工具解析与超时重新生成并通过的可播放产物：

- `integrations/local-ai-audio-rust/.acceptance/indextts25-1788063676903634000-30439-0/sample.wav`
- `integrations/local-ai-audio-rust/.acceptance/voxcpm2-1788063723472578000-30909-0/sample.wav`

两条新产物的时长、格式、SHA-256 与非静音指标和原始证据完全一致。回传的 `process_log_tail` 已验证不含安装/输出绝对路径。两套本地服务在验收前后均未运行；smoke 直接使用各自隔离环境和本地模型，不修改安装、模型、配置或正式输出目录。

## 事实、推断与未知

### 事实

- 两套安装、模型标记和隔离 Python 运行时在本机可发现。
- 两个固定 smoke 都完成了真实模型加载、生成、`ffprobe` 识别和 `ffmpeg -xerror` 全解码。
- 产物有非零音量，不是空白 WAV。
- 本次没有调用 Fish 111、HeyGen、云端生成、付费接口或用户正式文案，也没有重新克隆个人声音。
- 共 10 个确定性测试覆盖状态协议、HTTP ready 仍为 `end_to_end=not_run`、marker 冒充目录不被 discovery/缺参 CLI 自动执行、敏感环境不继承、超时 kill+wait、过量输出失败、日志脱敏、输出冲突拒绝和失败产物清理。

### 推断

- 两个模型至少对这条固定短句和已记录参数在当前 MPS 环境可用。
- 由于服务当前未运行，桌面导演台若采用常驻 WebUI/Gradio 接线，还需要生命周期管理；这不影响本次直接本地 smoke 的 E2E 结论。

### 未知

- 长文本、并发、连续多次生成的稳定性尚未验证。
- 音色自然度和听感未做人工主观验收；技术通过不等于内容质量通过。
- Gradio API 的具体请求协议与版本兼容性尚未纳入本层；本次只验证固定回环服务身份和直接模型 smoke。

## 总装接线说明

总装任务可把该 crate 作为 Tauri Rust 侧 path dependency，再将 `ProbeResponse`、`AudioVerification` 与 `SmokeReport` 原样序列化返回前端。建议保持以下边界：

1. 前端 smoke 必须同时传 Provider 枚举和用户本次明确批准、经过桌面目录选择器得到的安装路径；禁止把 discovery 排序结果直接升级为执行授权，也不允许传 shell/URL/端口/可执行路径。
2. `status=ready` 与 `end_to_end=passed` 分开显示；不得用服务健康替代生成验收。
3. 正式生成另设受限请求结构，限制文本长度、输出目录和参数白名单；不要复用 smoke 固定句接口承载业务文案。
4. 服务启动命令与媒体工具由 Tauri/Rust 内部按 Provider 或固定候选映射，不从前端接收可执行文件或参数数组。
5. 本分支没有修改 `docs/development/macos-director-acceptance-matrix.md`、Tauri 壳、Web 或 Go 代码。产品级待测试/版本文档由唯一总装负责人在合并后统一更新，避免并行分支冲突。
