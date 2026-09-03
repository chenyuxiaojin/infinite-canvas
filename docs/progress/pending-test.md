---
title: 待测试
description: 当前版本已实现但仍需人工验证的变更项
---

# 待测试

## 画布运行时性能

- 平移/缩放改为直接改世界层 DOM，停手后再回写 React；滚轮已合并到 rAF。
- 拖节点期间只改被拖节点和连线的 transform/path，松手才 `setNodes`。
- 节点 `memo` 的回调和面板改为稳定引用；缩小到 0.2 以下不再解码原图/原视频。
- 打开画布不再一次性 hydrate 全部媒体，只加载视口内节点；视频 `preload="none"`，同时只允许一条在播。
- 画布 persist 按项目分片写入 IndexedDB，视口/侧栏改动不改 `updatedAt`、不触发云端全量 PUT。
- 桌面 Go/Node sidecar 改为并行启动；切到后台标签会停任务轮询；助手面板改为动态加载；`ProConfigProvider` 只留在管理后台。

## macOS Tauri 2 桌面壳

- 新增 Apple Silicon macOS 桌面构建，复用现有 Next.js standalone 和 Go API，不改写 React 画布。
- 桌面版 Node/Go sidecar 只监听 `127.0.0.1`，参数和资源路径由 Rust 固定，网页 capability 不包含 shell 权限。
- SQLite、后端日志和 WebKit 画布数据使用应用专属目录；开发模式和生产 `.app` 已完成本机启动、退出重启及项目恢复验证。
- 画布已用确定性测试素材完成本地 FFmpeg 任务回填、文本/图片/视频/音频节点、来源连线、ZIP 导出回灌、播放、重启恢复和重复防护验收。
- 现阶段仅供本机开发验收；技术 DMG 已生成并校验完整，但本机缺少 Developer ID Application 身份，发行签名、公证、stapling、Gatekeeper 干净安装与覆盖升级被精确阻塞。
- Node sidecar 的最小发行 entitlement 已验证为 `com.apple.security.cs.allow-jit`；最终签名必须移除开发态 `get-task-allow`，其余主程序、Go 与 Sharp 组件不需要额外 entitlement。
