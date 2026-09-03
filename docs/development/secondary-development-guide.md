# 无限画布自主分支二次开发与治理指南

> 状态：生效中。
> 适用仓库：`github.com/chenyuxiaojin/infinite-canvas`（本地路径：`~/项目/自己的应用/infinite-canvas`）。

---

## 一、 仓库定位与分支模型

1. **唯一事实主干**：
   - 远程：`git@github.com:chenyuxiaojin/infinite-canvas.git`
   - 本地仓库定位为 **macOS 原生专业桌面端导演编排台**。
   - 所有特性分支通过 worktree 独立开发与验证（位于 `../infinite-canvas-worktrees/`）。

2. **与上游的关系（采石场模式，Cherry-pick Only）**：
   - 上游：`git@github.com:tigerowo/infinite-canvas.git`
   - **绝不对上游执行全量 `git merge upstream/main`**：上游已演进为公网 Web SaaS 多租户服务，全量合并会破坏本机的 Tauri 桌面运行时、本地哈希资产引用与人机协作协议。
   - **单向吸收机制**：上游发布新版本时，通过查看 `git log upstream/main`，仅针对纯前端无害的小交互、小动效建立临时 cherry-pick 分支（如 `feat/upstream-060-cherrypick`）单向吸收。

3. **开源协议边界**：
   - 本项目派生自上游 MIT License 阶段（commit `57b13aa`），底层受 MIT 保护。
   - 本应用仅作为 macOS 本地单机应用运行，所有数据与回环端口（3100/3101/3102）绑定 `127.0.0.1`，无任何公网 AGPL 网络传染风险。

---

## 二、 核心架构规范

1. **唯一公共操作接口（Single Source of Truth）**：
   - 所有对画布工程的变更，无论是人类操作、内置轻量助手、外部 CLI 还是 TUI AI，必须经过 `canvas-operation-protocol.ts` 转换成 `CanvasOperationBatch`。
   - 绝不允许绕过协议直接向 SQLite `project_data` 写入无 revision、无锁的裸数据。

2. **本地媒体零公网泄露原则**：
   - 画布媒体使用 `local-ref:{asset_id}` 体系，经过 SHA-256 校验并存放在应用私有目录。
   - 高清音视频播放依靠 Tauri 本地 HTTP Range 流式服务，不依赖任何云端 S3/OSS。

3. **资金风控铁律（Paid Generation Gate）**：
   - 任何 Agent 发起的付费生成调用（如 Seedance/H3 视频生成）必须先落为 `pending_approval` 任务卡片，必须经人类在界面上手动点击“批准”才触发真实网络请求扣费。

---

## 三、 模块目录映射

| 路径 | 语言 | 职责 |
| :--- | :--- | :--- |
| `desktop/src-tauri/` | Rust | macOS 原生外壳、Local Media Range 流媒体、PTY 终端、进程守护 |
| `integrations/` | Rust | 外部专业软件联动（DaVinci Resolve、Eagle、本地 FFmpeg、端侧音频） |
| `web/` | Next.js/React | Tauri WebView 渲染的前端界面（导演工作台、无限画布、xterm.js 终端） |
| `main.go`, `service/`, `handler/` | Go | 本机回环后端（SQLite 存储、提示词动态调度、本地资产管理） |
