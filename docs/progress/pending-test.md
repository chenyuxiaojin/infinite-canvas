---
title: 待测试
description: 当前版本已实现但仍需人工验证的变更项
---

# 待测试

## 桌面端本机素材收编（拖放 / 临时文件 / 自动找回）

- 从 Finder 或 macOS 截图缩略图把图片/视频拖进画布：节点应落在松手位置；截图会被移进
  `<画布素材目录>/画布素材/`，Finder 里原位置不再有该文件；项目目录里的文件保持原地引用。
- 「添加本机素材」弹窗新增「画布素材目录」行，可选择/更换；未设置时临时文件收进应用内
  `project-media/owned/`，提示文案会说明。
- 把已引用的项目内文件挪到同目录树的别处或改名，重新打开画布应自动显示并提示「已按内容找回」，
  再次打开不再扫描（引用已写回工程）。同名不同内容的文件不能被误认。
- Bridge/CLI 侧行为不变；`local-media-roots.json` 新增 `project_media_dirs` 字段。
- 已补 Rust 单测（临时目录判定、收编去重与重名、按摘要找回）；按项目约定未跑端到端 UI 自动化。

## 大画布历史与桌面同步性能

- UI 拖动、缩放或编辑节点时，`node.update` 只记录实际变化的字段；仅移动节点不再把
  节点中的完整长提示词重复写入审计历史。
- 工程加载和每次操作后自动压缩协作历史：保留最近 20 条人工操作、最近 100 条
  Agent/System 操作及其幂等记录；累计裁剪数量及最近裁剪 revision 写入 `history`，
  工程当前 revision、节点、连线、锁和任务不变。
- 桌面画布打开后每 500ms 只读取 SQLite 行的 `updated_at`；只有更新时间变化时才读取
  完整工程，不再持续解析和跨进程传输大 JSON。
- 已补充长提示词差异、2400 条旧历史压缩、审计满额后的 Agent 批次识别，以及桌面
  轻量轮询不解析工程 JSON 的回归用例；按项目约定本轮未执行自动化测试或构建。
- 人工验收重点：打开历史较大的「案例2-美甲师日常 EP01」静置、拖动和编辑提示词，
  对比 CPU/内存与交互延迟；首次正常编辑或 Agent 写入后确认 SQLite 中工程体积下降，
  CLI 仍能读取、修改并按 revision 处理冲突。

## 人与 Agent 共用画布协议与共编界面

- `CanvasProject` 新增向后兼容的操作状态，本地、远程、ZIP 导入导出仍使用同一工程 JSON。
- UI 节点/连线改动和内置 Agent 结构操作经由同一 reducer，记录 actor、request ID、project ID、base revision、时间、结果和错误。
- 重复 request 不重复执行；过期 revision 明确拒绝；Agent 不能改动人工锁定节点；冲突批次原子回滚。
- 成功 Agent 批次保存可持久化撤销快照；已有后续人工修改时拒绝直接快照恢复，避免覆盖人工结果。
- 共编界面提供 Agent 状态、revision、审计历史、人工锁与最近 Agent 修改提示；界面只映射 `operationState`，不另存 reducer、revision、锁或撤销状态。
- 自动化覆盖公共协议、Store 与共编状态映射；隔离桌面 Bridge 闭环证据及正式旧
  包尚未切换的边界见总装 handoff 和统一验收矩阵。

## Bridge 白名单扩容（图片/视频/生成配置节点）

- 白名单新增 `create_image_node` / `create_video_node` / `create_config_node`；
  媒体节点只接受已验收的受控 `local-ref:` 引用（结构校验 + 桌面运行时确认资产
  在受管根内存在且摘要一致，失败返回 `MEDIA_REFERENCE_UNAVAILABLE` 且不建节点），
  配置节点只收 model/size/count 白名单字段，metadata 形状与 UI 配置节点一致。
- dry-run 与 apply 都先做引用存在性预检；web reducer 零改动（`node.create` 泛化 +
  既有 `localMedia` 形状校验兜底）。
- 自动化：canvas 单测 9（含映射与穿越/错 MIME/错 key 负例）、contract 12（含
  白名单媒体节点端到端与不可用引用拒绝）。统一 bundle 已实机复测：真实受控
  图片/视频/配置节点可见，dry-run 不落库，重放幂等。实机发现的伪 asset ID
  身份绑定缺口已修复；复测返回 `MEDIA_REFERENCE_UNAVAILABLE`，revision/节点数不变。

## 付费生成执行器（H3 图生视频 + 人工批准闸门）

- 协议层新增 `pending_approval` 任务状态与人工专属 `task.approve` 操作；reducer 级
  不变量：Agent 发起且 `details.paid` 为真的 `task.start` 必须以 `pending_approval`
  提交，否则整批拒绝——花钱闸门不依赖 UI 约定。拒绝复用 `task.cancel`（待批准任务
  直接进 `cancelled`）。
- Bridge 新增 `POST /v1/generation/video-requests` 与 CLI `generation video request`：
  校验提示词/分辨率(768P/2K)/时长(4-15s)/关键帧引用后，单个原子批次创建视频占位
  节点 + 来源连线 + `pending_approval` 任务，并从桌面配置报出预计成本；不调用任何
  付费 API。能力清单声明 `paid: true, approval_required: true`，显式拒绝项由
  `paid_generation` 改为 `unapproved_paid_generation`。
- 节点卡显示「待批准 · 预计 ¥X」+ 提示词摘要 + 批准/拒绝按钮；批准走 Tauri 命令：
  人工 `task.approve` 批次落库后，Rust 驱动线程调 H3（`/v2/video_generation` 提交、
  `/v2/query/video_generation/{id}` 轮询、下载）→ 结果进 agent-media inbox → 复用
  既有 executor 转码 + ffprobe/解码验收 → system 批次回填任务终态与 `local-ref:`
  媒体引用。驱动在 Rust 侧运行，UI 切走不中断；App 退出会中断执行中的任务（停留
  running，需人工取消后重新提交）。
- H3 凭据存应用支持目录 `paid-generation/config.json`（0600，模板自动生成）：
  base_url/api_key/model/price_yuan_per_second；Bridge 不回显 key，任务快照与节点
  数据不含供应商 URL 与凭据。
- 自动化：web 协议 19 tests（含批准生命周期与闸门不变量）、Agent contract
  12 tests（含付费请求端到端）、桌面 crate 19 tests。真实 H3 闭环已在动作前获得用户
  单次授权，只提交 1 次（¥0.54）且未自动重试；下载、ffprobe、完整解码、Range、
  system 回填、重启与无敏感泄漏检查均通过。视觉上存在首帧到提示词场景跳变，
  该片只用于技术验收，不作生产采用。

## 受控图片摄入（Agent 关键帧静图）

- Bridge 新增 `POST /v1/media/image-ingests` 与 CLI `media image ingest`：只接受固定
  inbox 内的 `.png/.jpg/.jpeg/.webp` basename + 小写 SHA-256，上限 100 MiB；目录穿越、
  符号链接、摘要不匹配、零尺寸/不可解码内容均结构化拒绝。
- 图片验收为同步流程：校验 + 哈希 + 内容寻址拷贝进 `verified/` + ffprobe 尺寸探测
  在请求内完成，单个 agent 原子批次直接建成品 `image` 节点（含 `local-ref:` 引用与
  `localMedia` 元数据），不留 canvas task。
- 已验收副本按内容寻址；inbox 文件被清理后，相同请求重放仍返回同一引用。
- Bridge 响应不含播放 URL；UI 打开时经既有 `localMedia` 补水走 3103 Range 流显示。
- `probe_media` 改为从可信目录（/opt/homebrew/bin、/usr/local/bin、/usr/bin）解析
  ffprobe，修复 GUI 启动的 App 因无 shell PATH 而探测不到媒体尺寸的问题。
- 自动化：桌面 crate 19 tests（含摄入校验矩阵与无 inbox 重放）、Agent contract
  11 tests（含图片摄入端到端与 CLI 子命令）。案例 1 真实 `S01-全景剪影.png`
  （2048×1152）已完成隔离实机摄入、尺寸探测、内容寻址、inbox 删除后幂等重放、
  无凭据 401、四端口 loopback、UI 显示和重启保留。本轮未再执行导出回灌；其边界
  已由本机媒体 v5 及 v3/v4 兼容验收覆盖。

## 本机 Agent 适配层

- 新增只监听 `127.0.0.1:3102` 的 Agent Bridge，以及桌面安装专属、
  `0600` 保存、可立即撤销替换的本机凭据。
- 新增 `infinite-canvas` CLI：能力目录、项目列表/读取、画布操作
  dry-run/apply、运行时探测、任务状态/取消和零付费确定性测试片。
- Agent 写入带 project/request/base revision/actor，使用同一
  `operationState.requests` 幂等；revision 冲突和人工锁定节点均拒绝覆盖。
- 桌面 WebView 改由 Tauri IPC 与 Agent 共用现有 SQLite
  `canvas_projects` 表，首次加载会合并原 IndexedDB 项目。
- 画布库显示本机数据库与 Agent Bridge 的连接状态；IPC 失败时明确提示当前只有
  浏览器本地数据、Agent 无法直接读写，不再只在控制台记录。
- 自动化已覆盖 loopback、鉴权、撤销、白名单、路径 schema、幂等、JSON 和
  CLI 退出码；仍需在总装后的签名 `.app` 中人工确认 CLI 打包位置、
  首次项目合并和长时间并发编辑体验。

## macOS Tauri 2 桌面壳

- 新增 Apple Silicon macOS 桌面构建，复用现有 Next.js standalone 和 Go API，不改写 React 画布。
- 桌面版 Node/Go sidecar 只监听 `127.0.0.1`，参数和资源路径由 Rust 固定，网页 capability 不包含 shell 权限。
- SQLite、后端日志和 WebKit 画布数据使用应用专属目录；开发模式和生产 `.app` 已完成本机启动、退出重启及项目恢复验证。
- 画布已用确定性测试素材完成本地 FFmpeg 任务回填、文本/图片/视频/音频节点、来源连线、ZIP 导出回灌、播放、重启恢复和重复防护验收。
- 现阶段仅供本机开发验收；技术 DMG 已生成并校验完整，但本机缺少 Developer ID Application 身份，发行签名、公证、stapling、Gatekeeper 干净安装与覆盖升级被精确阻塞。
- Node sidecar 的最小发行 entitlement 已验证为 `com.apple.security.cs.allow-jit`；最终签名必须移除开发态 `get-task-allow`，其余主程序、Go 与 Sharp 组件不需要额外 entitlement。
