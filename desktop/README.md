# 小陈的画布 · macOS 桌面版

个人分支从 `1.0.0` 独立维护；界面只显示本机版本记录，不查询上游更新。
保留上游 MIT 声明、内部 CLI 名称及 `com.chenyuxiaojin.infinitecanvas` 应用身份，
因此改名不更换数据库、WebKit、收藏或已有配置目录。

本轮已安装并启动 Rust API，安装身份、数据读回和仍待完成的原生操作见 [整合验收](../docs/progress/canvas-rust-repair.md)。

此目录将现有 Next.js standalone 页面服务和 Rust 业务 API 作为固定 sidecar
装入 Tauri 2。网页源码仍在 `web/` 独立维护和构建；桌面壳不复制业务
前端，也不向网页开放任意 shell。

## 本机构建

```bash
cd desktop
bun install --frozen-lockfile
PATH="${HOME}/.cargo/bin:${PATH}" bun run dev
PATH="${HOME}/.cargo/bin:${PATH}" bun run build:app
```

`bun run dev` 会先完整执行资源暂存，再启动 Tauri 开发模式；
`beforeBuildCommand` 在生产打包前调用同一个 `bun run stage`。暂存
过程依次构建 `web`、`infinite-canvas` Agent CLI、arm64 Rust 业务 API sidecar，
从当前 Node 24 运行时提取 arm64 sidecar，并把 Next standalone 资源
放入忽略目录。当前 P1 只验收 Apple Silicon macOS；未来增加其他架构时，
必须分别生成带目标三元组后缀的 sidecar。

本机更新请先正常退出 App，再执行 `bun run build:app`（构建并安装）；
已有构建包时可执行 `bun run install:app`。安装器校验 App 身份和本机签名，
将旧版压缩到仓库旁的 `infinite-canvas-backups/local-installs/` 并验证 ZIP，
再把旧安装包移入废纸篓、新构建包移到 `~/Applications/小陈的画布.app`。
首次改名安装会识别旧的 `~/Applications/无限画布.app`，按同样流程替换，
并仅更新指向旧/新正式 App 的 `~/.local/bin/infinite-canvas` 链接；独立 CLI 不覆盖。
如果新旧 App 同时存在则停止，请先确认正式版本。打开片子的终端时，已有项目配置
会由原绑定流程更新到新 App 内的 MCP 命令路径；不批量改其他 Agent 的全局配置。
不保留散落的可运行备份，不修改应用数据，不重置整个 macOS 应用数据库。
失败时恢复旧安装包；App 仍运行时拒绝替换。单独执行 `tauri build` 只构建，
会留下开发产物，不是完成安装的流程。本机签名不代表可对外分发或已公证。

桌面 Node 使用 `background-node.cjs` 预加载器，只在此 macOS 子进程中将
`process.title` 改名保留在 JS 内，避免 Node 24 的改名实现触发 Launch Services
登记为第二个 Dock 应用；不修改系统 Node，不创建额外终端，不影响后台服务。

右侧终端只运行普通登录 Shell；用户自行输入 Agent 或其他命令，不预设启动按钮。
输出采用单一有序二进制 Channel，由 xterm 流式解码 UTF-8；调整尺寸按帧合并，
字符行列未变化时不通知 PTY。改项目标题/主题不重启会话，关闭或切项目回收 Shell。
当前正式 1.0.0 已收敛为本地工作流：App 登录、账号同步与云存储入口停用；
画布、我的素材和提示词收藏保存在本机，用户自配 AI API 及提示词在线目录保留。
旧账号、服务端素材/工作流和浏览器存储原样保留，不自动清理或迁移；
此项已完成正式安装、重启和数据对账，详见 `../docs/progress/local-workflow-install-acceptance.md`。
终端中 AI 工具自身的登录及模型用量仍由各工具管理，未在此分支移除。

侧栏“本机 Codex”试接已正式安装并完成一条真实只读验收，使用已安装的官方 Codex CLI 和现有 ChatGPT 登录；
不把认证信息交给前端，也不改全局配置。只认现有明确片子绑定，独立保存画布对话与
Codex 会话对应关系。此连接关闭 Shell、外部 MCP/插件、电脑操作及内置生图，
画布工具经动态工具接口执行；删除/媒体生成另行确认。上下文显示基于最近一次用量，
不是账号余额。验收证据见 `../docs/progress/canvas-codex-install-acceptance.md`，
连续对话、媒体确认及真实上下文压缩仍待测。这里的“本机”指连接程序在本机，模型推理仍使用 Codex 云端服务。

运行时固定拓扑：

- Tauri WebView：`http://127.0.0.1:3100`
- Next standalone：只监听 `127.0.0.1:3100`
- Rust API：只监听 `127.0.0.1:3101`
- Agent Bridge：只监听 `127.0.0.1:3102`，使用应用数据目录内的安装专属凭据
- SQLite 与后端日志：Tauri 应用数据目录

端口被占用时桌面壳直接报错退出，不连接未知进程。前端 capability
不包含 shell 权限；sidecar 的可执行文件、参数和工作目录全部由 Rust
固定。Agent Bridge 不开放 shell、自由路径或公网监听，详细命令和
总装边界见 [本机 Agent 适配层](../docs/development/local-agent-adapter.md)。App 资源
保留上游 MIT 原作者声明，并包含所打包 Node.js v24.12.0 的许可证。
