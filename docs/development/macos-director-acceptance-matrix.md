# macOS 桌面导演台验收矩阵

本文档是从上游无限画布改装为小陈个人 AI 视频生产桌面导演台的执行与验收单。所有“通过”必须附可复现命令或人工操作证据；未真实执行的项目保持“待验证”，不得用推断代替。

## 固定边界

- 目标平台：macOS，Tauri 2 桌面壳；保留 React/Next.js 网页独立构建能力。
- 本地优先：SQLite/项目目录保存状态和媒体；不要求公网服务，不上传大媒体。
- Rust 负责受控本地执行核心；原 Go 后端先保留，出现明确桌面阻碍后再提出替换方案。
- 本地控制只允许 Tauri IPC 或 `127.0.0.1` 受控接口；路径和命令必须白名单化或来自用户明确选择。
- 验收只使用测试素材或确定性短样例；不渲染或导出正式用户视频。
- Fish 111 是独立付费 Provider，未经单次授权不得调用；不得把敏感凭据写入源码、日志、提交、产物或文档。
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
| P2.1 | 只通过 Tauri IPC 或 `127.0.0.1` 受控接口通信，不向公网暴露本机控制端口 | 通过 | 当前总装只注册 7 个封闭 IPC：运行时探测、桌面/画布固定测试片、任务状态、受限媒体读取、取消、用户原生选择后的 ZIP 保存；capability 只向固定 `http://127.0.0.1:3100/*` 放行这些命令与 `core:default`，无 shell 权限。真实 `.app` 的自有监听仅为 Node `127.0.0.1:3100` 与 Go `127.0.0.1:3101`，未新增控制端口；`lsof -nP -a -p <app/sidecar pid> -iTCP -sTCP:LISTEN` 现场复核 |
| P2.2 | 命令/路径采用白名单或用户明确选择，画布不能执行任意 shell | 通过 | 任务 IPC 请求没有命令、可执行路径、URL、Host、端口或参数数组字段；画布固定样例只写应用支持目录内注册根，媒体读取再次校验根目录、文件名、大小和 SHA-256。ZIP 导出只接受二进制 ZIP，经原生保存框明确选址，并用同卷 staging + `hard_link` 原子发布且拒绝覆盖。执行核心 17 个测试覆盖路径穿越、符号链接越界、shell 元字符、输出冲突、取消/超时和持久化故障；桌面 8 个测试覆盖固定请求、隐私字段、回环导航及导出拒绝覆盖/非 ZIP；`cargo clippy --locked --all-targets -- -D warnings` 通过 |
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
| P3.4 | 全项目导出/重新导入后节点关系和本地媒体不丢失 | 通过 | 桌面原生保存框导出 `data/p3-evidence/P3-workflow-bedaac2.zip`（忽略目录），ZIP 192945 bytes、SHA-256 `8803df0e961dcdde3f054390b3a1d20083e97823b22a95d8f0ebdb716c97b9b9`，`unzip -t` 通过；含 v3 `projects.json` 与 3 份媒体。回灌后仍为 4 节点/1 连线；内嵌视频/音频/图片 SHA 分别为 `d3bf7ba437acab289ed29638f3e481004c5828af84525c4e1d1c76d47fe1dddd`、`c6b6fb62aa740f601ad8fabd41ea0889087eec458e12aabdfb7d6635164005c3`、`51ceaa223006f44c00831e742b7740e682e2c29ba96312e6b898a523d0e2717e`，均与导出前一致并由 FFmpeg 完整解码。退出重启后回灌项目和三份媒体仍可打开/播放 |
| P3.5 | 原始资料、选中结果、失败结果和日志边界清楚，不覆盖用户文件 | 通过 | 用户测试图片/音频只作为独立 Blob 存储，不修改源文件；执行结果只发布到 `~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/local-executor/acceptance/` 的哈希化固定文件名，画布只取得二次 SHA 校验后的受限副本。失败 partial 会清理，任务事件不记录路径/参数，导出只能经原生选择写 `.zip` 且 create-new 语义拒绝覆盖；ZIP 清单区分 `projects.json`、各项目 `files/` 和 storageKey |
| P3.6 | 人工与内置 Agent、打包 CLI/Bridge、system task 使用同一工程、revision、锁和审计，可见、可测、可撤销 | 进行中 | 隔离验收 App `com.chenyuxiaojin.infinitecanvas.integrationtest` 使用固定 `127.0.0.1:3210/3211/3212` 和一次性工程完成 dry-run、Agent 创建、UI 即时显示、重复 request、人工锁、`LOCKED_NODE`、`STALE_REVISION`、重读续写、UI 撤销、重启和 ZIP 往返；协议 13 passed，共编 UI 7 passed。用户现场实际运行的仍是主检出 `e00acb7` 旧包，只监听 3100/3101、正式 SQLite `canvas_projects` 为 0 行且没有 3102 Bridge，因此旧包不能直接供 Agent 读写。正式包切换和备份后首次迁移尚未由用户确认，隔离结果不能代替。 |
| P3.7 | CLI 随 `.app` 打包；Bridge 只监听 loopback；凭据不泄露；ZIP 往返保留结构和协议状态 | 通过 | 标准 `.app/Contents/MacOS/infinite-canvas` 为 arm64，SHA-256 `f4bcb2460c40d6bc917f5e76a60fe385150b3da92d05e65b2041ca1e25f32138`，与 release CLI 一致；最新技术包已安装到 `~/Applications/无限画布.app`，稳定 CLI 入口为 `~/.local/bin/infinite-canvas`，尚未自动启动或迁移正式项目。`lsof` 现场只见验收 App 在 `127.0.0.1:3210/3211/3212` 监听；capabilities 明确拒绝 shell、任意路径/URL、付费生成和公网监听。安装凭据为私有文件，按真实 secret 扫描 tracked files 和标准 App 可执行文件无命中。真实 UI 导出 ZIP 198566 bytes、SHA-256 `07dc9e984af0e2e4d8b22f9c3171dc287069015f7ffb1cd5cf7df13f6b302024`，`unzip -t` 无错误；修复导入身份重绑定后，副本 `tTxP4FnkV59R4J75I8AYF` 保留 revision 10、4 节点、2 连线、1 锁、1 task、12 条 audit，所有 audit/result project ID 均为副本 ID，重放原 request 返回 `duplicate:true` 且持久 revision 仍为 10。 |

### P3 事实、推断、未知

- 事实：真实 `.app` 已完成文本/图片/视频/音频画布、固定本地任务回填、来源连线、播放、ZIP 导出/回灌、退出重启与重复防护。项目 JSON 和大媒体 Blob 分离保存，但 v3 ZIP 会用 storageKey manifest 将三份媒体一起打包；哈希与完整解码均已现场复核。
- 事实：原网页 `file-saver` 在 Tauri WebView 内点击后没有文件或错误。桌面分支因此改为 raw binary IPC + macOS 原生保存框，用户明确选址后由 Rust 校验 ZIP、同步 staging 并原子 create-new 发布；浏览器构建仍沿用原下载行为。
- 事实：公共 `CanvasProject.operationState` 已接管人、Agent 和 system 的数值 revision、锁、task、request 幂等及 audit；Bridge 不再初始化第二张 Agent journal 表。真实总装验收曾暴露并修复媒体 `blob:` 恢复产生伪 revision、桌面轮询水合竞态、导入副本历史 ID 未重绑定和高 revision 被较新墙钟误拒绝四类问题。
- 事实：用户现场运行的是基线主检出旧包，不含 Agent Bridge；它的工程仍在
  WKWebView IndexedDB，正式 SQLite 工程表为空。隔离 bundle 的 P3.6 通过不能
  证明这个旧包已完成升级。
- 推断：当前安全边界足以承载后续本地 Provider 的同类“固定请求 -> 结构化状态 -> 验证后回填”工作流；真实生产任务仍应先扩展任务类型和输入白名单，而不是开放 shell 或任意路径。
- 未知：同 bundle ID 的新技术包首次启动后，正式旧 IndexedDB 工程迁移到 SQLite
  的结果尚未由用户在备份后确认；当前固定任务约 1 秒，取消行为由真实执行核心
  自动测试而非人工抢按按钮验收；超大项目 ZIP 仍是内存构建并设置 2 GiB IPC
  上限，性能/流式导出尚未验证。达芬奇/Eagle 写入和正式用户视频不在本阶段范围内。

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
