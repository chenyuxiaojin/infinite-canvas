---
title: 人与本机 Agent 共用无限画布总装 Handoff
description: 三分支总装决策、公共接口、真实验收证据与发行接线
---

# 人与本机 Agent 共用无限画布总装 Handoff

## 分支与组成

- 总装分支：`feat/human-agent-canvas-integration`
- 基线：`feat/macos-director-console` 的 `e00acb7`
- 核心协议来源：`bf3e2f2022ffcb77bfd0aebae955ec378d9b9c0f`
- 本机 Agent 适配来源：`6e4af0903957af3ce09e423a8f299657589a6210`
- 人机共编 UI 来源：`1354555a76d810296b898b486b4cb22ef5bd630b`
- 独立 worktree：`infinite-canvas-worktrees/human-agent-integration`

最终提交以本分支 HEAD 为准；不在本 worktree 合并回主线。

## 唯一公共接口

唯一画布修改语义是：

```text
web/src/app/(user)/canvas/protocol/canvas-operation-protocol.ts
  migrateCanvasProject
  applyCanvasOperationBatch
  buildCanvasStructureOperations
  rebindCanvasProjectIdentity
```

所有调用者都修改同一 `CanvasProject`：

- 人工 UI：`useCanvasStore.updateProject` 把结构差异转为 `actor: human` 批次；
  标题、锁和撤销也进入公共协议。
- 内置 Agent：动作入口调用 Store 的 `applyOperationBatch`。
- 外部 CLI/Bridge：`CanonicalCanvasAdapter` 把白名单操作映射成
  `CanvasOperationBatch`，调用固定 loopback Next 内部端点；`project.create`
  也用同一 reducer 从 revision 0 创建 revision 1 的 SQLite 工程。
- 本地任务：发起为 human/agent `task.start`，执行结果为 system
  `task.update`，可在同批回填节点。
- 受控视频摄入：Bridge 只接收应用私有 inbox 内的 MP4 文件名和小写 SHA-256，
  在同一工程先创建 `video` 节点与 canvas task，再交给现有 Rust executor；
  ffprobe 结果由同一个 system 批次直接回填 task 终态以及节点的稳定
  `local-task:` 引用、尺寸、时长和摘要，不建立第二套媒体画布，也不要求 UI 已打开。

在包含本总装代码的桌面包内，持久化唯一来源是 SQLite
`canvas_projects.project_data.operationState`，其中包含数值 revision、locks、
tasks、requests 和 audit。WebView 通过 Tauri IPC 读写这些行并轮询当前打开
工程；内部 Next 端点是无状态 reducer 执行器。浏览器独立构建和旧桌面包仍会
使用 IndexedDB，不能把“隔离验收包通过”解释成旧桌面包已经自动获得 Bridge。

## 冲突决策与删除项

### UI 冲突

- `canvas-client-page.tsx` 保留原交互与 React 本地渲染状态，但外部工程版本
  水合期间设置同步栅栏，禁止旧节点视图回写成“人工删除”。
- `use-canvas-store.ts` 保留 IndexedDB/账号同步兼容路径；桌面合并先比 revision，
  revision 相同才比 `updatedAt`，无差异 patch 不再制造保存。
- 画布库明确显示“本机数据库与 Agent Bridge 已连接”；Tauri IPC 补水或保存失败
  会显示错误状态，不再只写浏览器控制台后静默表现成可供 Agent 使用。
- `canvas-collaboration-adapter.ts` 只从 `operationState` 映射状态、历史和节点标记；
  不拥有 reducer、revision、锁或 undo 快照。
- system 对 Agent 媒体批次的 runtime task ID、进度和验收结果回填不会使该 Agent
  批次失去可撤销性；任何后续 human/agent 结构修改仍会阻止旧批次撤销。
- UI 打开 headless 已完成的视频节点时只把稳定 `local-task:` 引用物化为内存中的
  Blob URL；不会补写 `bytes` 或其他项目字段，因此不会制造 human revision。
- 桌面画布库每秒从同一 SQLite 刷新，因此 CLI 新建工程无需重载或重新导入即可出现。

### Bridge 冲突

- 删除 `SqliteCanvasAdapter` 的独立 reducer、SHA revision、锁判定与 SQL journal。
- 删除 `agent_operation_requests` 建表和写入；幂等完全来自
  `operationState.requests`。
- Bridge 仍保留稳定 HTTP/CLI JSON、凭据、退出码和白名单能力；协议映射只做
  `create_text_node` 等外部命名到公共 operation 的转换。
- `save_human_project` 以 revision 为第一优先级，只有 revision 相同时才比较墙钟，
  避免较旧时间戳拒绝更高 revision。
- Next loopback reducer在 WebView 导航瞬间出现传输失败时最多重试 2 次；request ID
  使响应丢失后的重放仍然幂等，不会重复建节点或启动任务。

### 兼容与恢复

- 旧工程缺少 `operationState` 时迁移为 revision 0，不改节点和连线。
- 本地媒体持久化为稳定 `storageKey`，页面新建的 `blob:`/`data:` URL 不计为修改。
- ZIP v3 可继续导入，当前导出为 v4；导入副本换 ID 时同时重绑定 audit/result/request 指纹，保留锁、
  task、历史、连线和重复请求幂等。
- Agent 视频的 `local-task:` storageKey 进入 ZIP manifest；导出包含经哈希验收的
  MP4，导入沿用媒体 Blob 兼容路径。

## 安全边界

- 正式端口固定 `127.0.0.1:3100/3101/3102`；验收 feature 使用独立固定
  `127.0.0.1:3210/3211/3212`，不接受动态 host 或公网监听。
- Bridge 不开放 shell、可执行路径、任意路径、任意 URL、raw SQL 或付费生成。
- Agent 不能提交路径：`GET /v1/media/inbox` 只返回应用私有固定 inbox，摄入请求
  只能给 basename、`.mp4`、小写 SHA-256、项目/节点/request ID 与画布位置尺寸；
  目录穿越、符号链接、摘要不匹配、空文件和超过 1 GiB 均结构化拒绝。
- 安装凭据只写应用支持目录私有文件；CLI 不接受 token 参数。
- Tauri capability 只放行固定 WebView origin 和必要的桌面画布命令。

## 真实零付费验收

以下结果来自隔离 bundle `com.chenyuxiaojin.infinitecanvas.integrationtest`，证明
总装代码的数据流成立，但不证明基线主检出生成的旧 `无限画布.app` 已包含这些
代码。正式 bundle 的升级迁移必须单独验收，不能用本节代替。

复现配置：

```bash
cd desktop
PATH=/Users/chenhuajin/.cargo/bin:$PATH \
  bun run tauri build \
  --config src-tauri/tauri.integration.conf.json \
  --bundles app \
  --features integration-acceptance
```

隔离 bundle ID 为 `com.chenyuxiaojin.infinitecanvas.integrationtest`，不会读写
正式 App 的项目数据。真实结果：

1. UI 生成固定 1 秒本地测试片；human 创建节点/连线和 task，system 回填成功。
2. 打包 CLI 读 capabilities/工程；dry-run 不落库；apply 创建 Agent 节点/连线；
   已打开 UI 自动显示；同 request 重放不重复。
3. UI 锁定后 CLI 返回 `LOCKED_NODE`；人工 revision 前进后旧请求返回
   `STALE_REVISION`；重读后可修改未锁节点。
4. UI 撤销最近 Agent 批次；重启后 revision 10、锁、task、audit 和结构仍在。
5. 原生保存框导出并重新导入；副本保留 4 节点、2 连线、1 锁、1 task、
   12 audit，历史身份与幂等已重绑定到副本 ID。
6. 监听仅 loopback；标准 App 内 CLI 与 release CLI 哈希一致；真实凭据内容未在
   tracked files 或标准 App 可执行文件中出现。
7. 新增媒体增量在同一隔离 bundle 完成：CLI 在已打开画布库时创建工程，卡片无需
   重载即出现；固定 inbox MP4 经 dry/哈希边界后生成 640x360、2005 ms 视频节点，
   UI 自动出现并可播放。项目稳定在 revision 4，只有 1 个 node、1 个 canvas task、
   4 条 audit；重放同 request 后四项计数均不变。
8. 人工锁定视频节点后 CLI 得到 `LOCKED_NODE`；人工解锁使 revision 5→6 后，旧
   base revision 得到 `STALE_REVISION`。应用退出重启后 revision 6、节点、任务、
   审计和媒体仍在；重复摄入返回 `duplicate:true` 并报告当前 revision 6。
9. 真实导出 `/tmp/agent-media-final-export.zip` 为 514 KiB，包含 `projects.json`
   与 491694 bytes 的 MP4。首次回灌暴露 WKWebView 原生选择框把组合 MIME/后缀过滤
   误判为不可选；输入过滤收窄为 `.zip` 后重包复测，Open 恢复可用并成功导入副本
   `waCbl0WxWvM8Ro1OjHYYa`。副本为 revision 6、1 视频节点、1 task、8 audit，历史
   project ID 全部重绑定，UI 仍显示 00:02 并可播放。
10. 独立撤销工程的 Agent 媒体批次在两次 system 回填后仍显示“完成 · 可撤销”；
    用户确认后从真实 UI 点击“撤销批次”，UI 变为“画布元素 0 / revision 5 /
    已撤销”。CLI 同步读回 0 node、0 connection、0 task、5 audit，原 ingest audit
    记录非空 `undoneByRequestId`。
11. 正式 App 的一次性工程 `冒烟-一次性工程（可删）` 暴露旧完工批次只有
    `task.update` 的 headless 缺口；打开 UI 后节点由 loading 恢复为 00:06 可播放，
    实际点击进入“暂停”状态，证明旧 UI 兜底有效，但也证明 CLI-only 不能算完整。
12. 修复后在全新隔离工程 `headless-h264-norev-20260831` 复测：工程从未在 UI
    打开，CLI 轮询任务后 SQLite 已为 revision 4、节点 `success`，最后一个 system
    批次同时包含 `task.update + node.update`，并具有稳定 content/storageKey、
    1344x768、6583 ms 和摘要；重复轮询不增加 revision。随后 UI 直接显示 00:06，
    点击进入“暂停”，CLI 再读仍为 revision 4、4 audit。H.264/AAC 输入 255441 bytes，
    stream-copy 输出 254832 bytes，ffprobe 与完整 `-xerror` 解码通过。

## 用户现场复核与交付断点

用户已在 2026-08-30 完成正式 bundle 的安全换装：先成对备份 Application
Support 与 WebKit 到
`~/项目/自己的应用/infinite-canvas-backups/backup-20260830-1815.tar.gz`，再启动
`~/Applications/无限画布.app`。现场确认 3100/3101/3102 均正常、未授权 Bridge
访问为 401、CLI 可列出迁移后的 4 个工程，并在一次性 P3 工程完成 dry-run、apply、
revision 0→1、幂等、审计与撤销快照。也就是“桌面 SQLite + Bridge 直写”已经由
用户实机复核，不再是旧包/空表状态。

正式 App 已完成受控 MP4 冒烟并由用户要求保留一次性工程和 inbox 副本等待确认删除；
没有向其他正式工程追加视频或任务。headless/H.264 修复的自动化和新闭环只写独立
bundle `com.chenyuxiaojin.infinitecanvas.integrationtest`。最终标准 App 会构建并暂存，
不在正式进程运行时覆盖安装。

## 验证命令与结果

```bash
go test ./...

cd web
bun run test:canvas-protocol
bun run test:collaboration
bun run build

cd integrations/local-agent-adapter-rust
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets

cd ../../desktop/src-tauri
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features

cd ..
PATH=/Users/chenhuajin/.cargo/bin:$PATH bun run tauri build --bundles app
```

最终硬门禁计数：Web 协议/Store/共编/本地媒体共 24 tests，本机 Agent crate
7 unit + 10 contract tests，桌面 crate 10 tests，本地 executor 19 tests（另有
1 个需显式 trusted FFmpeg 的测试默认 ignored，已显式运行通过）。Go 全部
package、Next 生产构建及标准 arm64 `.app` 构建通过。标准包内 CLI 与 release CLI
SHA-256 均为 `b4c5d126240451259252587829ab7cb77e517751b8323a39ea83d93d7afee197`；
真实 H.264 headless MP4 闭环已覆盖。

`bun x tsc --noEmit` 仍有 8 个基线错误，位于
`canvas-resource-references.ts`、`video-settings-panel.tsx`、`gemini.ts` 和
`canvas-agent.ts`；这些文件未因总装修改，Next 构建不受影响。

## 残留风险与发行接线

- 当前标准 `.app` 是技术构建；Developer ID、公证、staple 和干净机升级仍按
  P4 矩阵执行，不能把 ad-hoc 包当发行包。
- 用户当前正式包已包含受控视频增量，但仍是“UI 打开后补回填 + MPEG-4 Part 2
  重编码”的旧实现；headless/H.264 修正版不能在运行中覆盖，需用户退出后换装。
- 1 GiB 是协议硬上限而不是推荐镜头大小；当前 IPC 会把验收后的媒体复制进 WebView
  Blob，27 镜审片墙的总内存与滚动性能仍需用一次性副本压测，不能从单个 2 秒镜头
  推断大批量性能。
- 发行签名后再次核对 CLI 仍在 `Contents/MacOS/infinite-canvas`、所有内嵌
  Mach-O 的签名顺序、Bridge loopback 监听和凭据文件权限。
- 全仓 TypeScript 基线修复可另开任务；不应在本总装分支混入无关业务修正。
