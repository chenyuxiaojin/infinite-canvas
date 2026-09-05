# 小陈的画布

macOS 上的本地 AI 内容创作工作台。故事、人物、场景、分镜、原图、视频、音频和创作对话放在同一张可编辑画布中；一部片子对应一个明确目录和一张绑定画布。

基于 [infinite-canvas](https://github.com/tigerowo/infinite-canvas) 的个人分支，从 `1.0.0` 独立维护，保留 [MIT 许可证](LICENSE) 与原作者声明。上游下载和更新不代表这个分支的正式安装包。

## 核心能力

- 节点编辑、连线、分组、创作目录、平移缩放、撤销重做。
- 原图及历史素材、视频和音频引用，携带文件与校验值的项目 ZIP 导入导出。
- 用户自配 AI API，以及侧栏本机 Codex、Grok、Antigravity 连接；各来源的实测状态见专项报告。
- 本机保存、错误恢复、跨重启版本历史、明确目录绑定和普通 Shell 终端。
- 提示词目录搜索、按需正文、本机收藏；保留图片、视频、全景、导演台和创作工作流。

本轮修复与 Rust 后端已安装到正式 App，保存和原数据核对通过；副本导入、历史恢复及原生性能等验收仍在进行。[统一施工与验收状态](docs/progress/canvas-rust-repair.md)。

## 本机开发与安装

需要 Apple Silicon macOS、Node.js `24.12.0`、Bun 和 Rust 工具链。

```sh
cd web
bun install --frozen-lockfile
cd ../desktop
bun install --frozen-lockfile
PATH="$HOME/.cargo/bin:$PATH" bun run dev
```

正常退出 App 后，以 `desktop` 中的 `bun run build:app` 构建并更新唯一正式入口 `~/Applications/小陈的画布.app`。安装器先备份旧 App；实时业务数据备份、安装读回与回退步骤见 [桌面说明](desktop/README.md)。

当前正式 App 使用 React / TypeScript 前端、Next standalone 页面服务，以及 Rust 桌面壳、业务 API 和 Agent Bridge。旧 Go 源码作为迁移对照保留，正式运行不再依赖 Go sidecar。SQLite 与浏览器 IndexedDB 保留本机项目及原素材。

## 文档

- [产品需求与完成标准](docs/overview/product-requirements.md)
- [文档索引](docs/index.md)
- [待测试](docs/progress/pending-test.md) · [TODO](docs/progress/todo.md)
- [数据与历史优化](docs/progress/canvas-data-optimization.md) · [画布与素材优化](docs/progress/canvas-ui-optimization.md)
- [本机 Agent 接入证据](docs/progress/canvas-local-agent-integration.md)
- [上游说明存档](docs/overview/upstream-readme-reference.md)（含上游原作者、服务与部署信息，不作为本分支运行指南）
