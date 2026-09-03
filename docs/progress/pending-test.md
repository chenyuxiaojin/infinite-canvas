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

## 本机 Agent 适配层

- 新增只监听 `127.0.0.1:3102` 的 Agent Bridge，以及桌面安装专属、
  `0600` 保存、可立即撤销替换的本机凭据。
- 新增 `infinite-canvas` CLI：能力目录、项目列表/读取、画布操作
  dry-run/apply、运行时探测、任务状态/取消和零付费确定性测试片。
- Agent 写入带 project/request/base revision/actor，使用 request journal 幂等；revision 冲突和人工锁定节点均拒绝覆盖。
- 桌面 WebView 改由 Tauri IPC 与 Agent 共用现有 SQLite
  `canvas_projects` 表，首次加载会合并原 IndexedDB 项目。
- 自动化已覆盖 loopback、鉴权、撤销、白名单、路径 schema、幂等、JSON 和
  CLI 退出码；总装后的签名 `.app` 已确认 CLI 打包位置、首次项目合并和真实片子目录绑定。
- 已完成 MCP STDIO 实测：四个工具可发现，`canvas_context` 读取案例2为 17 节点/10 连线，单节点 `canvas_read` 只返回该节点，`canvas_mutate` dry-run 后数据库数量不变。
- 已完成右侧本地终端实测：案例2目录自动连接画布，选中节点会出现上下文标签并以安全粘贴方式进入输入区；Codex 能在侧栏启动并带入无限画布 MCP 配置。
- 仍需后续长时间观察多人/多 Agent 连续编辑；Claude Code 新项目 MCP 的首次信任确认由用户在首次使用时完成。

## macOS Tauri 2 桌面壳

- 新增 Apple Silicon macOS 桌面构建，复用现有 Next.js standalone 和 Go API，不改写 React 画布。
- 桌面版 Node/Go sidecar 只监听 `127.0.0.1`，参数和资源路径由 Rust 固定，网页 capability 不包含 shell 权限。
- SQLite、后端日志和 WebKit 画布数据使用应用专属目录；开发模式和生产 `.app` 已完成本机启动、退出重启及项目恢复验证。
- 画布已用确定性测试素材完成本地 FFmpeg 任务回填、文本/图片/视频/音频节点、来源连线、ZIP 导出回灌、播放、重启恢复和重复防护验收。
- 现阶段仅供本机开发验收；技术 DMG 已生成并校验完整，但本机缺少 Developer ID Application 身份，发行签名、公证、stapling、Gatekeeper 干净安装与覆盖升级被精确阻塞。
- Node sidecar 的最小发行 entitlement 已验证为 `com.apple.security.cs.allow-jit`；最终签名必须移除开发态 `get-task-allow`，其余主程序、Go 与 Sharp 组件不需要额外 entitlement。
