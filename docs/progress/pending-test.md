---
title: 待测试
description: 当前版本已实现但仍需人工验证的变更项
---

# 待测试

## 无限画布人与 Agent 共编

- 组件和状态测试已覆盖人工锁阻止 Agent、revision 漂移阻止覆盖和撤销、文本批次字段级撤销、媒体/删除不可逆，以及状态/批次历史的可访问文字。
- 1440×900 浏览器零付费验收已覆盖：本地 Agent demo 新建可编辑节点、`Agent 刚修改` 标识、人工锁、历史摘要与影响节点、最近批次撤销和后续人工 revision 保护。
- 仍需在统一操作核心总装后，用真实但零付费的 Agent 动作流复验 adapter 接线；本分支没有调用付费模型，也没有实现 CLI、Agent HTTP 服务或 Rust 执行器。

## macOS Tauri 2 桌面壳

- 新增 Apple Silicon macOS 桌面构建，复用现有 Next.js standalone 和 Go API，不改写 React 画布。
- 桌面版 Node/Go sidecar 只监听 `127.0.0.1`，参数和资源路径由 Rust 固定，网页 capability 不包含 shell 权限。
- SQLite、后端日志和 WebKit 画布数据使用应用专属目录；开发模式和生产 `.app` 已完成本机启动、退出重启及项目恢复验证。
- 画布已用确定性测试素材完成本地 FFmpeg 任务回填、文本/图片/视频/音频节点、来源连线、ZIP 导出回灌、播放、重启恢复和重复防护验收。
- 现阶段仅供本机开发验收；技术 DMG 已生成并校验完整，但本机缺少 Developer ID Application 身份，发行签名、公证、stapling、Gatekeeper 干净安装与覆盖升级被精确阻塞。
- Node sidecar 的最小发行 entitlement 已验证为 `com.apple.security.cs.allow-jit`；最终签名必须移除开发态 `get-task-allow`，其余主程序、Go 与 Sharp 组件不需要额外 entitlement。
