---
title: 待测试
description: 当前版本已实现但仍需人工验证的变更项
---

# 待测试

## 人与 Agent 共用画布协议与共编界面

- `CanvasProject` 新增向后兼容的操作状态，本地、远程、ZIP 导入导出仍使用同一工程 JSON。
- UI 节点/连线改动和内置 Agent 结构操作经由同一 reducer，记录 actor、request ID、project ID、base revision、时间、结果和错误。
- 重复 request 不重复执行；过期 revision 明确拒绝；Agent 不能改动人工锁定节点；冲突批次原子回滚。
- 成功 Agent 批次保存可持久化撤销快照；已有后续人工修改时拒绝直接快照恢复，避免覆盖人工结果。
- 共编界面提供 Agent 状态、revision、审计历史、人工锁与最近 Agent 修改提示；界面只映射 `operationState`，不另存 reducer、revision、锁或撤销状态。
- 自动化覆盖公共协议、Store 与共编状态映射；隔离桌面 Bridge 闭环证据及正式旧
  包尚未切换的边界见总装 handoff 和统一验收矩阵。

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
  12 tests（含付费请求端到端）、桌面 crate 19 tests。仍需实机验证：真实 H3 一笔
  付费生成闭环（预计 ¥0.5 左右）。

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
  11 tests（含图片摄入端到端与 CLI 子命令）。仍需实机验证：真实关键帧 PNG 从
  inbox 摄入后画布显示、缩放与导出回灌。

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
