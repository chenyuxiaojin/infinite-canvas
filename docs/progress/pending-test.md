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
- 自动化覆盖公共协议、Store 与共编状态映射；桌面 Bridge 真实闭环证据见总装 handoff 和统一验收矩阵。

## 本机 Agent 适配层

- 新增只监听 `127.0.0.1:3102` 的 Agent Bridge，以及桌面安装专属、
  `0600` 保存、可立即撤销替换的本机凭据。
- 新增 `infinite-canvas` CLI：能力目录、项目列表/读取、画布操作
  dry-run/apply、运行时探测、任务状态/取消和零付费确定性测试片。
- Agent 写入带 project/request/base revision/actor，使用 request journal 幂等；revision 冲突和人工锁定节点均拒绝覆盖。
- 桌面 WebView 改由 Tauri IPC 与 Agent 共用现有 SQLite
  `canvas_projects` 表，首次加载会合并原 IndexedDB 项目。
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
