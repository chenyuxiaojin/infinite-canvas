---
title: 待测试
description: 当前版本已实现但仍需人工验证的变更项
---

# 待测试

## macOS Tauri 2 桌面壳

- 新增 Apple Silicon macOS 桌面构建，复用现有 Next.js standalone 和 Go API，不改写 React 画布。
- 桌面版 Node/Go sidecar 只监听 `127.0.0.1`，参数和资源路径由 Rust 固定，网页 capability 不包含 shell 权限。
- SQLite、后端日志和 WebKit 画布数据使用应用专属目录；开发模式和生产 `.app` 已完成本机启动、退出重启及项目恢复验证。
- 画布已用确定性测试素材完成本地 FFmpeg 任务回填、文本/图片/视频/音频节点、来源连线、ZIP 导出回灌、播放、重启恢复和重复防护验收。
- 现阶段仅供本机开发验收；技术 DMG 已生成并校验完整，但本机缺少 Developer ID Application 身份，发行签名、公证、stapling、Gatekeeper 干净安装与覆盖升级被精确阻塞。
- Node sidecar 的最小发行 entitlement 已验证为 `com.apple.security.cs.allow-jit`；最终签名必须移除开发态 `get-task-allow`，其余主程序、Go 与 Sharp 组件不需要额外 entitlement。
