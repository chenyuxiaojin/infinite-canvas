# macOS 桌面导演台验收矩阵

本文档是从上游无限画布改装为小陈个人 AI 视频生产桌面导演台的执行与验收单。所有“通过”必须附可复现命令或人工操作证据；未真实执行的项目保持“待验证”，不得用推断代替。

## 固定边界

- 目标平台：macOS，Tauri 2 桌面壳；保留 React/Next.js 网页独立构建能力。
- 本地优先：SQLite/项目目录保存状态和媒体；不要求公网服务，不上传大媒体。
- Rust 负责受控本地执行核心；原 Go 后端先保留，出现明确桌面阻碍后再提出替换方案。
- 本地控制只允许 Tauri IPC 或 `127.0.0.1` 受控接口；路径和命令必须白名单化或来自用户明确选择。
- 验收只使用测试素材或确定性短样例；不渲染或导出正式用户视频。
- Fish 111 和 MiniMax-H3 均是独立付费 Provider，未经单次授权不得调用；
  不得把敏感凭据写入源码、日志、提交、产物或文档，付费超时不得自动重试。
- 分发方向为 Developer ID 签名、公证 DMG，不以 Mac App Store 为首发渠道。
- 保留 MIT 许可证、原作者声明、前端标识和 `upstream` remote。

## 状态定义

- `通过`：已有命令输出、文件哈希、可解码产物或可重复的人工操作证据。
- `进行中`：已开始且已有部分证据，但通过标准尚未全部满足。
- `阻塞`：已定位到明确外部依赖或权限缺口，并记录原始证据。
- `待验证`：尚未执行，不代表失败。

## P0 Fork 与原版基线

| ID | 通过标准 | 状态 | 证据/复现入口 |
| --- | --- | --- | --- |
| P0.1 | 当前登录 GitHub 身份创建 fork；记录 fork URL、upstream、默认分支和锁定提交 | 通过 | fork `https://github.com/chenyuxiaojin/infinite-canvas`；upstream `https://github.com/tigerowo/infinite-canvas`；默认分支 `main`；锁定提交 `57b13aa1a2d7439955b0e65abe742bc7144df32f` |
| P0.2 | 独立克隆、干净工作树、产品分支、保留 upstream | 通过 | 目录 `/Users/chenhuajin/项目/自己的应用/infinite-canvas`；分支 `feat/macos-director-console`；`origin` 为个人 fork，`upstream` 为原仓库；P0 文档提交后用 `git status --short --branch` 复核 |
| P0.3 | 完整读取上游文档、许可证和项目规则 | 通过 | 已读根 `AGENTS.md`、`README.md`、`LICENSE`、`CHANGELOG.md`、`VERSION` 及 `docs/` 下全部 17 份 Markdown；MIT 原声明为 `Copyright (c) 2026 tigerowo` |
| P0.4 | 原版依赖安装、后端测试、前端构建、前后端最小启动冒烟通过 | 通过 | Go 1.27.0、Bun 1.3.5、Node 24.12.0；`go mod download` 与 `bun install --frozen-lockfile` 成功；`go test ./...` 全部通过；`bun run build` 成功生成 19 个页面/路由；`go run .` 与 `bun run dev` 启动后，后端和前端代理的 `/api/health` 均返回 HTTP 200 `ok`，首页 HTTP 200、标题“无限画布” |
| P0.5 | 创建项目，保存并在重启后恢复；导出后重新导入且数据完整 | 通过 | Playwright 在原版 UI 新建项目与文本节点 `P0 基线持久化测试 57b13aa1`；正常停止并重启 Go/Next 后，原项目 ID 与节点内容恢复；导出 ZIP 后删除原项目，再导入得到新项目 ID，仍为 1 节点/0 连线且文本一致。ZIP `5f5f1fa265ba37bbb624a2666d2d3ecd526fd5ae0e101a9b8fea617288b151f0`，截图 `2759d6a420e1b0dd505769e54ea01230f83e3c2feedb58bf2d9802e5f3329264`；本机证据在忽略目录 `data/p0-evidence/` |

### P0 事实、推断、未知

- 事实：fork、远端、分支和锁定提交已现场验证；上游许可证为 MIT；Go 测试、Next 生产构建、前后端启动、重启恢复和 ZIP 往返均已实际通过。真实 ZIP 格式为 v3，根目录含 `projects.json`，媒体按需放入同一 ZIP；上游部分文档中“JSON 导入/导出”的描述已落后于代码和界面。
- 推断：P1 不能只把当前网页静态导出后塞入壳，因为生产构建存在动态 `/api/[...path]` 和 `/canvas/[id]` 路由；需要用真实桌面启动证据选择 Next standalone/sidecar 或经过验证的静态化方案。
- 未知：原版 ZIP 对图片、视频、音频、导演台本地模型的完整往返覆盖仍未做；按矩阵留到 P3 使用多媒体测试画布验收。

## P1 Tauri 桌面壳

| ID | 通过标准 | 状态 | 证据/复现入口 |
| --- | --- | --- | --- |
| P1.1 | Tauri 2 启动现有画布并生成真实 macOS `.app` | 通过 | Tauri Rust `2.11.5`、CLI `2.11.4`、Rust `1.92.0`、Node `24.12.0`；`PATH="/Users/chenhuajin/.cargo/bin:$PATH" bun run tauri build --bundles app` 成功生成 `desktop/src-tauri/target/release/bundle/macos/无限画布.app`，并可用 `open` 启动。主程序、Go、Node 均经 `file` 确认为 arm64 Mach-O；主程序 SHA-256 `d1425e8dcd4806970bc8ed1aeebae4c7348efe7ca5f8b5ebb4d812370b79a7a5` |
| P1.2 | 无公网服务器时可打开画布并管理本地项目 | 通过 | App 内只由 Node `127.0.0.1:3100` 与 Go `127.0.0.1:3101` 监听；以无效外部代理并对 `127.0.0.1,localhost` 直连的进程环境启动后，首页、`/api/health`、画布库和已保存项目均可用，节点 `P1 桌面重启持久化 bdc1e556` 可读取；未调用任何生成 Provider |
| P1.3 | 开发/生产资源路径、退出重启、数据持久化通过 | 通过 | 删除忽略的暂存资源后，`PATH="/Users/chenhuajin/.cargo/bin:$PATH" bun run dev` 会先重新暂存再启动 debug App，两个回环服务与动态画布路由均为 HTTP 200；生产 `.app` 新建项目 ID `10tinFLXxfwOwlc1r583o` 和文本节点后以 Cmd+Q 退出，重启及最终重建 `.app` 后项目、1 个节点、0 条连线和文本均恢复。后端数据在 `~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/`，画布 IndexedDB 在 `~/Library/WebKit/com.chenyuxiaojin.infinitecanvas/`；截图 SHA-256 `01f0b2a0d6537c4a0a37e8a33c29b171a95567b849ce338adbd9eaacd317be1e`，本机证据在忽略目录 `data/p1-evidence/` |
| P1.4 | React 网页层仍可单独构建 | 通过 | `web` 的 `bun run build` 多次独立成功，Next 16.2.9 仍生成 19 个页面/路由，包括动态 `/api/[...path]` 与 `/canvas/[id]`；桌面层复用 standalone 产物，未改 React 业务源码 |

### P1 事实、推断、未知

- 事实：Tauri 以固定参数启动打包的 Node standalone 与 Go sidecar，WebView 只允许导航到 `http://127.0.0.1:3100`；capability 没有 shell 权限。Go 新增可选 `BIND_HOST`，默认仍保持原有 `:PORT` 网络行为，桌面壳才显式绑定回环地址。端口被占用时 App 会在启动任何 sidecar 前输出明确错误并以状态 1 退出。Node v24.12.0 许可证随 App 打包，未写入凭据。
- 推断：保留 Next/Go 并由 Tauri 管理生命周期，已经满足 P1 的浏览器复用和本地桌面资源路径；P2 的独立 Rust 模块可在审查后通过受限 Tauri IPC 接入，无需把 React 或 Go 整体重写。
- 未知：当前只生成 Apple Silicon arm64 App，Intel/universal 发行尚未处理；正式产品名和新图标尚未选择，P1 只沿用上游“无限画布”和现有 Logo；Developer ID、Hardened Runtime、公证、DMG 与干净安装仍属于 P4。当前未提供签名身份时 bundle 显示 ad-hoc，`codesign --verify --deep --strict` 退出 1，不得把它当作可分发签名。

## P2 Rust 本地执行核心（框架与只读探测）

| ID | 通过标准 | 状态 | 证据/复现入口 |
| --- | --- | --- | --- |
| P2.1 | 只通过 Tauri IPC 或 `127.0.0.1` 受控接口通信，不向公网暴露本机控制端口 | 通过 | Tauri capability 只放行封闭桌面命令与固定 WebView origin，无 shell 权限。正式端口为 Node/Go/Bridge/媒体流 `127.0.0.1:3100/3101/3102/3103`；验收为 3210–3213。媒体流端口需要每进程随机能力凭据并支持 Range；`lsof` 实测全部只监听 IPv4 loopback，退出后四端口全部释放。 |
| P2.2 | 命令/路径采用白名单或用户明确选择，画布不能执行任意 shell | 通过 | 任务 IPC 没有命令、可执行路径或任意 URL。媒体只接受用户原生选择、受控 asset ID、inbox basename 或已授权 root ID 内相对路径；绝对路径、目录穿越、符号链接越界、摘要不匹配、Origin/凭据错误和大小越界均拒绝。工程只保存 root ID/相对路径/SHA/元数据；私有 root registry 为 0600。桌面 17 tests、本地执行核心 19 tests 与 Agent contract 覆盖这些边界，Clippy `-D warnings` 通过。 |
| P2.3 | FFmpeg 版本探测和确定性短样例处理通过，输出可完整解码 | 通过 | 真实 `.app` 面板识别 FFmpeg/ffprobe 8.1；从按钮生成任务 `f1aa691f-f2e6-473d-9ca1-2fc7adbc55dd`，产物在应用支持目录，1.000 秒、138603 bytes、320x180 MPEG-4 + 48 kHz 单声道 AAC；`ffprobe` 和 `ffmpeg -v error -xerror -i ... -map 0 -f null -` 通过，SHA-256 `d3bf7ba437acab289ed29638f3e481004c5828af84525c4e1d1c76d47fe1dddd`。退出重启后再次提交复用同一任务，媒体数和 journal 任务数均仍为 1，mtime/hash 不变 |
| P2.4 | Eagle 健康检查和只读探测，不修改素材 | 通过 | 真实 `.app` 与独立 CLI 均返回 `available`、V2 API 可达且已有 library context。生产 allowlist 只请求 `GET http://127.0.0.1:41595/api/v2/library/info`，800 ms 超时、64 KiB 上限；报告只保留上下文布尔值，没有请求 item/tag/folder/file 或任何写端点，也没有保留库名、路径和素材信息 |
| P2.5 | 达芬奇可用性/连接只读探测，不改工程、不渲染 | 通过 | 真实 `.app` 返回“已安装但未运行”；标准脚本模块与库存在。Provider 在确认进程未运行后即停止，没有启动 Resolve、没有执行 bridge、没有打开/修改工程或渲染；运行中 bridge 仅含五个固定读取方法并有 3 秒/每流 16 KiB 边界，21 个连接器测试通过。实时版本/工程/时间线状态因 Resolve 未运行而保持未知，不伪造连接成功 |
| P2.6 | IndexTTS/VoxCPM 服务与模型健康检查；仅真实可用时生成短测试音频并完整解码、人工可播放 | 通过 | 独立 Provider 探测确认 IndexTTS-2.5 16/16、VoxCPM2 5/5 模型标记及 Python 3.11.13；服务实时均为 `not_running`，未把 HTTP/界面当 E2E。显式批准安装路径后两套本地模型各执行一次固定短句 smoke，SHA-256 分别为 `010ff48713a84b18db54db38a732c9ab5fe61246c5acdd152e73bb93862b0559`、`eabb115d87ac173f8fa08deeb1eb23fe3e2a64d28f14a71e1e216290b04a82a2`；均有非零音量、`ffprobe` 与完整 `ffmpeg -xerror` 通过，且用 macOS `afplay` 各播放一次退出 0。没有 Fish 111、云端或付费调用 |
| P2.7 | 本地模型统一 Provider 接口；未连接时明确显示不可用状态 | 通过 | 两套声音 Provider 使用协议版本 1 和统一 `not_found/discovered/ready/not_running/model_missing/incompatible/error` 状态；真实 `.app` 面板显示 FFmpeg/Eagle“可用”、Resolve/IndexTTS/VoxCPM“未运行”，声音卡片明确显示“安装路径未授权”，不读取 Documents 安装目录、不显示假成功。普通探测的 `end_to_end` 固定为 `not_run`，只由真实 smoke 报告 `passed` |

### P2 事实、推断、未知

- 事实：三个独立模块经回归修复、独立复审后合入；执行核心 17 个单元测试加 1 个真实 FFmpeg 测试、连接器 21 个测试、声音 Provider 10 个测试全部通过；P3 总装后桌面接线增至 8 个测试，相关 Clippy 均以 `-D warnings` 通过。真实 `.app` 已完成状态探测、固定样例生成、完整解码、退出重启恢复和重复任务防护。
- 事实：首次 GUI 验收准确暴露了两处总装问题：自定义命令未加入 Tauri ACL，以及打包 App 自动扫描 HOME 后在 macOS Documents 隐私边界阻塞。前者已收窄为四命令权限；后者改为仅探测固定回环服务，安装目录一律等用户通过原生选择器授权，界面明确显示“安装路径未授权”。
- 事实：Eagle 实时只读健康检查可用；Resolve 已安装但未运行；IndexTTS/VoxCPM 本地服务均未运行。两套声音模型的直接本地 smoke 已真实通过，但桌面面板不会把这份历史 E2E 证据冒充当前服务就绪。
- 推断：当前封闭 IPC、应用自有输出根、结构化任务状态和 fail-closed Provider 状态足以作为 P3 画布任务回填的底座；正式项目媒体根仍必须来自原生目录选择或应用自有项目目录。
- 未知：Resolve 运行时的实时脚本连接、当前工程/时间线状态和 3 秒边界尚未在本机实连；两套声音服务的 Gradio/API 协议与长期稳定性未接入；声音自然度没有主观质量验收。当前 FFmpeg 使用受信任的本机安装，随 App 捆绑、逐层签名和公证留到 P4。

## P3 真实桌面工作流验收

| ID | 通过标准 | 状态 | 证据/复现入口 |
| --- | --- | --- | --- |
| P3.1 | 测试画布包含文本、图片、视频、音频及生成分支 | 通过 | 真实 `.app` 建立 `P3 本地工作流验收 bedaac2`：文本来源、FFmpeg 视频结果、确定性 PNG、确定性 WAV 共 4 节点，文本到视频 1 条连线；原项目 `b82ao8zI_5hiqAti6Dhy2`，回灌项目 `38h4O29tP8hOy96v2E0va`。重启后侧栏仍显示 4 类节点，视频实际进入“暂停”状态、音频实际进入 `Pause` 且进度为 0.204，证明 WebView 可播放 |
| P3.2 | 触发一次本地、安全、零付费的最小生产任务，结果回填节点并保留来源关系 | 通过 | 画布“本地测试片”调用固定 Rust/FFmpeg 请求，任务 `17c7aa81-7573-4355-bdbf-af884fd66648` 成功；应用目录产物 `canvas-test-clip-40df373e99b1783b.mp4` 为 1.000 秒、138603 bytes、320x180 MPEG-4 + 48 kHz 单声道 AAC，SHA-256 `d3bf7ba437acab289ed29638f3e481004c5828af84525c4e1d1c76d47fe1dddd`，`ffprobe` 与完整 `ffmpeg -xerror` 通过。结果以 `local-task:<task-id>` 写入视频节点并记录任务、SHA、来源节点；没有云端、Fish 111 或付费调用 |
| P3.3 | 失败、取消、重启恢复、重复任务防护和输出冲突都有可验证行为 | 通过 | 执行核心自动测试覆盖验证失败清理 partial、queued/running 取消、哈希阶段取消、进程失败/超时、重启将未完成任务标记失败、并发/串行幂等和 Reject 冲突；真实 `.app` 退出重启后任务仍成功且媒体可用。回灌项目点击“本地测试片”只聚焦既有节点并提示“该画布已保留本地测试片及来源关系”，journal 仍为 2 个任务、媒体仍为 2 个文件。桌面 ZIP 发布测试证明同名目标拒绝覆盖且原字节不变 |
| P3.4 | 全项目导出/重新导入后节点关系和本地媒体不丢失 | 通过 | 旧 v3/v4 ZIP 继续导入；v5 原生导出明确提供“嵌入媒体”和“仅引用/清单”。Rust 往返测试覆盖 v5 embedded 摘要一致、reference-only 不静默丢文件、v3/v4 迁移、重复路径/条目/总量边界和内容寻址幂等。原 P3 三媒体 v3 ZIP 哈希证据继续保留；真实 27 镜 v3 包回灌后变为 40 节点/27 视频/27 连线和 27 个受控 `project_copy` 引用。 |
| P3.5 | 原始资料、选中结果、失败结果和日志边界清楚，不覆盖用户文件 | 通过 | 用户测试图片/音频只作为独立 Blob 存储，不修改源文件；执行结果只发布到 `~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/local-executor/acceptance/` 的哈希化固定文件名，画布只取得二次 SHA 校验后的受限副本。失败 partial 会清理，任务事件不记录路径/参数，导出只能经原生选择写 `.zip` 且 create-new 语义拒绝覆盖；ZIP 清单区分 `projects.json`、各项目 `files/` 和 storageKey |
| P3.6 | 人工与内置 Agent、打包 CLI/Bridge、system task 使用同一工程、revision、锁和审计，可见、可测、可撤销 | 通过 | 隔离验收 App `com.chenyuxiaojin.infinitecanvas.integrationtest` 使用固定 `127.0.0.1:3210/3211/3212` 和一次性工程完成 dry-run、Agent 创建、UI 即时显示、重复 request、人工锁、`LOCKED_NODE`、`STALE_REVISION`、重读续写、UI 撤销、重启和 ZIP 往返。用户随后先备份正式 Application Support + WebKit，再启动 `~/Applications/无限画布.app`，现场确认 3100/3101/3102、未授权 401、4 个迁移工程、CLI dry-run/apply、revision 0→1、幂等、审计和撤销快照；正式 SQLite/Bridge 链路不再是空表。协议 15 passed，共编 UI 7 passed。 |
| P3.7 | CLI 随 `.app` 打包；Bridge/媒体流只监听 loopback；凭据不泄露；ZIP 往返保留结构和协议状态 | 通过 | 标准与验收 `.app` 均重新构建；包内没有 credential/database。`lsof` 现场只见验收 App 在 `127.0.0.1:3210/3211/3212/3213` 或正式端口组 `3100/3101/3102/3103` 的 IPv4 loopback 监听，退出后四端口全部释放。Bridge 凭据留在私有文件，媒体流使用另一个每进程随机能力凭据；脱敏 Range 证据不含凭据或路径。capabilities 明确拒绝 shell、任意路径/URL、未批准付费生成和公网监听。 |
| P3.8 | 外部 Agent 可安全创建一次性工程并把白名单目录 MP4 写成可播放视频节点，不接受任意路径 | 通过 | 新增 `POST /v1/projects`、`GET /v1/media/inbox`、`POST /v1/media/video-ingests` 及对应 CLI。隔离实测：画布库无需刷新即出现 CLI 新工程；固定 inbox 的 2 秒 MP4 生成 640x360、2005 ms 视频节点，UI 自动出现并可播放；稳定 revision 4、1 node、1 canvas task、4 audit，重复 request 计数不变。人工锁后为 `LOCKED_NODE`，人工 revision 5→6 后旧请求为 `STALE_REVISION`；重启后结构/任务/媒体仍在，重复摄入报告当前 revision 6。v4 ZIP 含 491694-byte MP4；修正 `.zip` filter 后副本 `waCbl0WxWvM8Ro1OjHYYa` 成功回灌，保留 revision 6、1 video、1 task、8 audit、重绑定历史及可播放 00:02 媒体。用户确认后从真实 UI 撤销独立媒体批次，UI 显示 0 元素/revision 5/已撤销；CLI 同步读回 0 node、0 connection、0 task、5 audit 和非空 `undoneByRequestId`。2026-08-31 正式 App 冒烟又发现“task 终态已写、节点需 UI 补写”及 H.264 被转 MPEG-4 Part 2 两个缺口；修复后全新隔离工程在从未打开 UI 时已由同一 system 批次写入 `task.update + node.update`，revision 4、节点 `success`、1344x768、6583 ms、稳定 `local-task:` 引用。255441-byte H.264/AAC 输入 stream-copy 为 254832 bytes，完整解码通过；打开并实际播放后 revision 仍为 4、audit 仍为 4。 |
| P3.9 | 本机已有媒体默认引用，不进 Blob/云端；Range、missing/relink、引用/复制和 ZIP 模式可验证 | 通过（内存回收留风险） | 分支 `feat/local-media-reference-streaming` 使用独立 bundle `com.chenyuxiaojin.infinitecanvas.localmediaacceptance`。真实 17,965,118-byte 27 镜副本导入为 27 个 `local-ref:`，持久化 `blob:`/绝对路径/播放 URL 均为 0；相同包再导入应用支持目录仍为 18,252 KiB。脱敏证据为 54 次 Range、54 次 206；真实播放、滚动、缩放后 operation revision 仍为 1。单测覆盖鉴权、206/Content-Range/416、路径/符号链接/SHA、missing/relink、reference/project_copy、v3/v4/v5。最终进程画布库 952,240 KiB，27% 打开稳定 1,115,216 KiB；离开 10 秒 1,115,456 KiB，未稳定回基线，因此不宣称内存/FPS 性能完全通过。App 退出后 3210–3213 全释放。 |
| P3.10 | 真实关键帧可受控摄入；统一 bundle 只允许已验收图片/视频引用与白名单配置，伪造资产不建节点 | 通过 | 案例 1 `S01-全景剪影.png`（2048×1152）在隔离工程完成 inbox + SHA-256 摄入、尺寸探测、内容寻址、inbox 删除后幂等重放、无凭据 401 与重启保留。统一 bundle 工程 `unified-whitelist-acceptance-v2-20260831` 的 dry-run 保持 revision 1/0 节点，apply 建立 image/video/config 三节点后 revision 2，幂等重放不增 revision。实机曾发现伪 asset ID 可沿用真路径/摘要的缺口；修复为 asset ID 必须匹配受控内容/路径身份后重包，同请求返回 `MEDIA_REFERENCE_UNAVAILABLE`，仍为 revision 2/3 节点/禁止节点 0。UI 初次打开当下 revision 仍为 2。后续审计记录真实 human 全景节点 create/update/delete 使 revision 2→8，结构回到 3 节点；用当前 base revision 8 再验伪引用仍被拒绝，revision 8/3 节点不变。 |
| P3.11 | Agent 付费任务必须待批准；人工单次批准后只调用一次，下载、解码、流式引用、system 回填、幂等和无敏感泄漏均有真实证据 | 通过（视觉结果不采用） | 用户在动作前明确批准这一次 MiniMax-H3 768P/6s（预计 ¥0.54）；供应商任务 `2094385551669305344` 仅提交一次，未自动重试。revision 7→8 是 human `task.approve`，后续四个 system 批次到 revision 12；终态 `succeeded/delivered`。产物 H.264/AAC、1344×768、6583 ms、595651 bytes，SHA-256 `639b8211b4d4a74c55ee77a0eeea35609f438406c40f1364e7399ff464b9dd62`；`ffprobe`、完整 `ffmpeg -xerror`、206/Content-Range、UI 播放、重启读回均通过。工程 JSON 无 key/供应商 URL/运行态播放 URL。但画面从桌前金钱剪影过渡到落地窗人物，首帧与提示词场景不一致，只判定技术链通过。 |

### P3 事实、推断、未知

- 事实：真实 `.app` 已完成文本/图片/视频/音频画布、固定本地任务回填、来源连线、播放、ZIP 导出/回灌、退出重启与重复防护。项目 JSON 和大媒体 Blob 分离保存，但 v3 ZIP 会用 storageKey manifest 将三份媒体一起打包；哈希与完整解码均已现场复核。
- 事实：原网页 `file-saver` 在 Tauri WebView 内点击后没有文件或错误。桌面分支因此改为 raw binary IPC + macOS 原生保存框，用户明确选址后由 Rust 校验 ZIP、同步 staging 并原子 create-new 发布；浏览器构建仍沿用原下载行为。
- 事实：公共 `CanvasProject.operationState` 已接管人、Agent 和 system 的数值 revision、锁、task、request 幂等及 audit；Bridge 不再初始化第二张 Agent journal 表。真实总装验收曾暴露并修复媒体 `blob:` 恢复产生伪 revision、桌面轮询水合竞态、导入副本历史 ID 未重绑定和高 revision 被较新墙钟误拒绝四类问题。
- 事实：Agent 视频完工现在由 Bridge 在同一批次提交 task 终态和节点稳定引用；UI
  仅物化临时播放 URL。H.264/AAC 输入走 stream copy，其他视频回退到 libx264/AAC，
  不再生成浏览器不兼容的 MPEG-4 Part 2。
- 事实：MiniMax-H3 付费闭环已用当下单次授权完成；仅提交 1 次、未自动重试，
  且从 human 批准到 system 四阶段回填全部进入同一 `operationState`。技术产物完整可解码
  且可 Range 播放，但首帧与提示词场景跳变，所以不能把技术通过写成视觉质量通过。
- 事实：统一 bundle 实机复测暴露并修复了伪 asset ID 可沿用真路径/摘要的身份
  绑定缺口；修复后伪引用结构化拒绝且不建节点。图片的空 `durationMs` 也在水合
  规范化时删除，真实 UI 打开后 revision 不再平白增加。
- 事实：用户已在成对备份后完成正式包换装；3102 Bridge、迁移工程、CLI 直写和
  一次性视频冒烟均由真实 UI 确认。headless/H.264 修复仍需换装新构建；其他正式
  用户工程未用于写入测试。
- 事实：隔离 App 已导入 17 MiB 的真实 27 镜审片包；40 节点/27 视频/27 连线全部
  可见，侧栏滚动、反复缩放和单片实际播放后 revision 仍为 1，结构计数不变且无
  白屏/崩溃。但相关进程总 RSS 从画布库 1,063,232 KiB 增至 1,303,968 KiB，退回
  画布库 10 秒后仍为 1,307,648 KiB；WebContent 单进程约为 1,013,456 KiB。退出
  隔离 App 后进程和 3210/3211/3212 均释放。
- 事实：上述 Blob 基线的后续实现已改为 `local-ref:` + 受鉴权 Range 流。全新隔离
  容器的 27 镜项目持久化检查为 Blob 0、绝对路径 0、播放 URL 0；54 次真实请求均为
  Range/206。引用/复制、missing/relink、v5 嵌入/仅引用、旧 v3/v4 和 Agent 边界均有
  自动化覆盖。3213 与原三端口同样只监听 loopback，退出后释放。
- 推断：当前安全边界足以承载后续本地 Provider 的同类“固定请求 -> 结构化状态 -> 验证后回填”工作流；真实生产任务仍应先扩展任务类型和输入白名单，而不是开放 shell 或任意路径。
- 未知：本轮仍没有 WKWebView 帧时间/FPS 仪表化，不能冒充 FPS 通过。Range 已消除
  WebView Blob/整段字节 IPC，但 metadata 预载与显式卸载后，最终进程离开页面 10 秒
  RSS 仍未稳定回到基线；需要后续视频节点虚拟化/缩略图与更长时长压力。v5 已在
  Rust 侧流式读写文件，但浏览器兼容导出仍保留旧内存路径。达芬奇/Eagle 写入和正式
  用户视频不在本阶段范围内。

## P4 macOS 分发验收

| ID | 通过标准 | 状态 | 证据/复现入口 |
| --- | --- | --- | --- |
| P4.1 | 盘点本机 Apple Developer/Developer ID 条件，不输出私钥或敏感信息 | 通过 | 仅聚合 `security find-identity -v -p codesigning` 的数量/类型：有效 identity 1，`Apple Development` 1，`Developer ID Application` 0，`Apple Distribution` 0；未输出身份哈希、证书内容、私钥或凭据。`notarytool 1.1.0 (39)`、stapler、Xcode 26.3 可用；环境中没有 Apple/notary 凭据变量名。Keychain profile 没有可安全枚举的标准入口，因此不猜 profile 名；Developer ID 缺失已是发行阻塞 |
| P4.2 | 所有内嵌可执行文件/辅助程序正确签名，启用 Hardened Runtime，明确 entitlements | 阻塞 | 未签发行包的 Go/main/Sharp 为 ad-hoc，官方 Node 虽为 runtime 签名但带 `com.apple.security.get-task-allow`，外层 `codesign --verify --deep --strict` 退出 1。临时副本逐个签 Sharp dylib/`.node`、Go、Node、主程序后再签外层，5 个 Mach-O 均为 `runtime`，深度严格校验通过；无 entitlement 的 Node 启动 Next 时退出 133：`Failed to reserve virtual memory for CodeRange`。只给 Node 增加 `com.apple.security.cs.allow-jit` 后，Next `127.0.0.1:3199` 和完整 App 均真实启动，Go/主程序/Sharp 保持无 entitlement，`get-task-allow` 已移除；最小清单在 `desktop/src-tauri/entitlements/node.plist`。最终 Developer ID 签名仍因本机 `Developer ID Application=0` 阻塞 |
| P4.3 | 生成 DMG，完成 notarization 与 stapling；缺权限时给精确证据 | 阻塞 | `bun run tauri build --bundles dmg` 成功生成 `desktop/src-tauri/target/release/bundle/dmg/无限画布_0.5.5_aarch64.dmg`，68704490 bytes，SHA-256 `28e8b5e7b433eee325f91216b480eb96758724a7206577158eee428a4124e24f`，`hdiutil verify` 为 VALID。该技术 DMG 未签名：`codesign` 退出 1，`spctl --type open` 退出 3 `source=no usable signature`，`stapler validate` 退出 65 `does not have a ticket stapled`。因缺 Developer ID，不提交必然无效的 notarization，也没有 submission ID |
| P4.4 | 干净安装路径验证 Gatekeeper、首次启动、覆盖安装和数据保留 | 阻塞 | DMG 已只读挂载，包内 App 深度签名校验和 Gatekeeper 均失败；本机直接从只读镜像可启动首页/画布库，并读取 P3 的两个 4 节点/1 连线项目，证明 bundle ID 数据目录未被技术重打包破坏。临时 Hardened Runtime 副本同样启动并保留数据。但没有 Developer ID、公证 ticket 和 quarantine 来源条件，不能把本机启动冒充 Gatekeeper 干净安装；未写入 `/Applications`，覆盖安装/正式升级保持阻塞 |
| P4.5 | 记录版本、提交、产物哈希和回滚方法 | 通过 | 技术候选版本 `0.5.5`、arm64、源提交 `7accabe0ea15cccb6848c8dd7196f898fe5c7e46`，DMG 哈希见 P4.3。回滚前先退出 App，并成对备份 `~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/` 与 `~/Library/WebKit/com.chenyuxiaojin.infinitecanvas/`；在独立 worktree 从前一总装提交 `bedaac2845b69d03310d598bb6ca4546c79c6b72` 重建旧 App，保持相同 bundle ID 后覆盖应用本体但不删除数据。若数据迁移不兼容，只能在 App 退出时成对恢复备份；当前没有执行破坏性回滚 |

### P4 事实、推断、未知

- 事实：首发分发目标是 Developer ID 签名、公证 DMG，不是 Mac App Store。本机没有 Developer ID Application 身份；当前 DMG 完整但未签、未公证、未 stapling，Gatekeeper 明确拒绝，因此它只能是技术验收产物，不能对外发行。
- 事实：包内共有 5 个 Mach-O：主程序、Go、Node、Sharp `.node` 和 libvips dylib。临时 ad-hoc 验证证明正确顺序是先签所有内嵌代码，再签外层 App；所有代码启用 Hardened Runtime，只有 Node 需要 `com.apple.security.cs.allow-jit`。没有该 entitlement 时出现可重复的 V8 CodeRange 失败；加入后 Next 和完整 App 均启动，证明不需要保留开发态 `get-task-allow`。
- 推断：获得 Developer ID 后应使用同样的逐层顺序与最小 Node entitlement，严格校验 App 后再签 DMG，随后用明确的 keychain profile 或 App Store Connect API key 运行 notarization、等待 Accepted、staple 并重新执行 `spctl`；当前不能从 ad-hoc 结果推断 Apple 服务会接受。
- 未知：Keychain 中是否另有未命名的 notarytool profile、Apple Team 是否有 Developer ID/公证权限、最终品牌名/图标、Intel/universal 产物和真正隔离机器上的升级行为。以上都需要用户提供外部条件或产品选择后才能继续。

## 阶段检查点

每阶段完成时必须执行：

1. 更新对应矩阵状态和证据。
2. 复核 `git status`，不得混入凭据、用户媒体、数据库或构建缓存。
3. 给出该阶段最新“事实、推断、未知”。
4. P0 完成后先汇报目录、分支和完整基线证据，再进入 P1。
5. 遇到付费调用、品牌名/图标选择、Eagle 或达芬奇真实数据写入时停止并请求确认。
