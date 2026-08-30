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
- 外部 CLI/Bridge：`CanonicalCanvasAdapter` 把六种白名单操作映射成
  `CanvasOperationBatch`，调用固定 loopback Next 内部端点。
- 本地任务：发起为 human/agent `task.start`，执行结果为 system
  `task.update`，可在同批回填节点。

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

### Bridge 冲突

- 删除 `SqliteCanvasAdapter` 的独立 reducer、SHA revision、锁判定与 SQL journal。
- 删除 `agent_operation_requests` 建表和写入；幂等完全来自
  `operationState.requests`。
- Bridge 仍保留稳定 HTTP/CLI JSON、凭据、退出码和白名单能力；协议映射只做
  `create_text_node` 等外部命名到公共 operation 的转换。
- `save_human_project` 以 revision 为第一优先级，只有 revision 相同时才比较墙钟，
  避免较旧时间戳拒绝更高 revision。

### 兼容与恢复

- 旧工程缺少 `operationState` 时迁移为 revision 0，不改节点和连线。
- 本地媒体持久化为稳定 `storageKey`，页面新建的 `blob:`/`data:` URL 不计为修改。
- ZIP v3 不变；导入副本换 ID 时同时重绑定 audit/result/request 指纹，保留锁、
  task、历史、连线和重复请求幂等。

## 安全边界

- 正式端口固定 `127.0.0.1:3100/3101/3102`；验收 feature 使用独立固定
  `127.0.0.1:3210/3211/3212`，不接受动态 host 或公网监听。
- Bridge 不开放 shell、可执行路径、任意路径、任意 URL、raw SQL 或付费生成。
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

## 用户现场复核与交付断点

现场复核时，实际运行进程来自主检出 `feat/macos-director-console` 的基线提交
`e00acb7`，只启动 Next `127.0.0.1:3100` 和 Go `127.0.0.1:3101`，没有
`127.0.0.1:3102` Agent Bridge；正式应用目录的 `canvas_projects` 为 0 行，画布
仍在该旧包的 WKWebView IndexedDB。外部 Agent 因此只能读到空数据库，不能把
操作直接写回已打开画布。

修复不是修改协议，而是交付并启动本分支构建的桌面包。首次启动时由新 WebView
读取同 bundle ID 下的旧 IndexedDB，经 Tauri IPC 写入 SQLite；迁移完成后必须
同时满足：画布库显示数据库/Bridge 已连接、CLI `projects list` 可见原工程、
SQLite 行数非零、已打开 UI 能看到 CLI 写入。为保护正式项目，自动化只在隔离
数据上执行；正式升级应先成对备份 Application Support 与 WebKit，再由用户确认
迁移结果。

本分支最新技术包已安装到 `~/Applications/无限画布.app`，稳定 CLI 入口为
`~/.local/bin/infinite-canvas`。现场旧包仍从主检出 worktree 运行；在用户明确允许
关闭旧包并迁移正式画布前，不自动启动新包。

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

以上硬门禁均通过：协议/Store 13 tests，共编 UI 7 tests，本机 Agent crate
7 unit + 5 contract tests，桌面 crate 8 tests，Go 全部 package，Next 生产构建
及标准 arm64 `.app` 构建。

`bun x tsc --noEmit` 仍有 8 个基线错误，位于
`canvas-resource-references.ts`、`video-settings-panel.tsx`、`gemini.ts` 和
`canvas-agent.ts`；这些文件未因总装修改，Next 构建不受影响。

## 残留风险与发行接线

- 当前标准 `.app` 是技术构建；Developer ID、公证、staple 和干净机升级仍按
  P4 矩阵执行，不能把 ad-hoc 包当发行包。
- 当前用户正在运行的仍是基线主检出旧包；必须切换到本分支技术包后，CLI/Bridge
  才存在。正式发布前用标准 bundle ID 再做一次旧正式工程的只读盘点和备份后
  迁移验收；自动化不触碰正式用户项目。
- 发行签名后再次核对 CLI 仍在 `Contents/MacOS/infinite-canvas`、所有内嵌
  Mach-O 的签名顺序、Bridge loopback 监听和凭据文件权限。
- 全仓 TypeScript 基线修复可另开任务；不应在本总装分支混入无关业务修正。
