---
title: 待测试
description: 当前版本已实现但仍需人工验证的变更项
---

# 待测试

## macOS Tauri 2 桌面壳

- 新增 Apple Silicon macOS 桌面构建，复用现有 Next.js standalone 和 Go API，不改写 React 画布。
- 桌面版 Node/Go sidecar 只监听 `127.0.0.1`，参数和资源路径由 Rust 固定，网页 capability 不包含 shell 权限。
- SQLite、后端日志和 WebKit 画布数据使用应用专属目录；开发模式和生产 `.app` 已完成本机启动、退出重启及项目恢复验证。
- 现阶段仅供本机开发验收；Developer ID 签名、公证、DMG 和干净安装尚未完成。
