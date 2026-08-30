# 本机 Agent 适配层总装交接

## 分支与基线

- 专属分支：`feat/local-agent-adapter`
- 起点：`e00acb71cf02dcc04e78f13ec76b63b6413a93d0`（`feat/macos-director-console`）
- 独立 worktree：`/Users/chenhuajin/项目/自己的应用/infinite-canvas-worktrees/local-agent-adapter`
- 本分支不合并其他功能分支，也不改写主检出。

## 已完成

- 独立 Rust crate 提供 loopback HTTP Bridge、机器可读 capabilities、安装凭据、撤销轮换、结构化错误和 CLI。
- Tauri 启动/停止 Bridge，并将现有 DesktopRuntime 的探测、零付费固定测试片、任务状态和取消适配到白名单 trait。
- 桌面 WebView 通过 Tauri IPC 使用现有 SQLite `canvas_projects`；
  外部 Agent 经 `CanvasOperationAdapter` 使用同一表。
- Agent 操作带 project/request/base revision/actor，支持 dry-run、compare-and-swap、持久幂等和人工锁定优先。
- CLI 作为 Tauri `externalBin` 随 `.app` 打包；不接受 token 参数，仅从私有凭据文件读取。
- React 画布 UI 未重做，只改了桌面持久化接线和首次本地项目合并。

## 总装顺序

1. 将本分支提交合入总装分支。
2. 若统一人/Agent 操作层尚未合入，保留 `SqliteCanvasAdapter`，功能可独立运行。
3. 若统一操作层已合入，按
   [本机 Agent 适配层](local-agent-adapter.md#总装接线与核心协议替换点)
   替换 `DesktopAgentBridge::start` 中的 adapter 构造。
4. 合并其他 Rust 集成分支时，手工保留 `desktop/src-tauri/Cargo.toml`
   的全部 path dependency，并重新生成 `desktop/src-tauri/Cargo.lock`；
   不要选择某一侧整文件覆盖。
5. 合并其他桌面启动分支时，保留端口预检 `3100/3101/3102`、Bridge 在
   Go AutoMigrate 后启动、退出时先停 Bridge 再停 DesktopRuntime 的顺序。
6. 合并画布 store 分支时，保留桌面分支优先：`isDesktopRuntime()` 时先读取桌面 IPC，再按 `updatedAt` 合并 IndexedDB；网页账号同步逻辑继续作为非桌面路径。
7. 执行本文件“验证命令”，再做签名 `.app` 人工验收。

## 精确替换点

统一操作层只需替换：

```text
desktop/src-tauri/src/agent_bridge.rs
  DesktopAgentBridge::start
    SqliteCanvasAdapter::open(...) -> canonical CanvasOperationAdapter
```

必须保持的外部协议：

- `GET /v1/capabilities`
- `GET /v1/projects`
- `GET /v1/projects/{project_id}`
- `POST /v1/canvas/operations/dry-run`
- `POST /v1/canvas/operations/apply`
- `GET /v1/runtime`
- `POST /v1/tasks/test-clips`
- `GET /v1/tasks/{task_id}`
- `POST /v1/tasks/{task_id}/cancel`
- `POST /v1/credentials/revoke`
- CLI 命令名、JSON envelope 和退出码

统一层接管后可删除的临时实现仅限：

- `SqliteCanvasAdapter` 内直接查询/更新 `canvas_projects` 的 SQL。
- `agent_operation_requests` 的建表和 journal SQL。

Bridge、CredentialStore、CLI、AgentRuntime trait、测试和文档不是临时替换对象。

## 验证命令

```bash
cd integrations/local-agent-adapter-rust
cargo test --all-targets

cd ../../desktop
bun install --frozen-lockfile
bun run stage

cd src-tauri
cargo test --all-targets

cd ../../../
go test ./...

cd web
bun install --frozen-lockfile
bun run build
```

`bunx tsc --noEmit` 在指定基线已有若干与本分支无关的错误；
报错文件均未被本分支修改。生产 `next build` 已通过，但总装若修复全仓
TypeScript 基线，可再把 `tsc --noEmit` 设为硬门禁。

## 未完成与人工验收

- 尚未把本分支自行合并进任何主线；应由总装任务处理。
- 技术 `.app` 已确认 CLI 位于 `Contents/MacOS/infinite-canvas`
  且为 arm64 Mach-O；仍需在总装后的发行签名 `.app` 中复核并验证
  `/usr/local/bin` 软链方案。
- 需要用真实旧版桌面 IndexedDB 项目人工确认首次合并和打开后编辑；自动化只验证 Rust 协议与独立 Web 构建。
- 没有调用付费模型，没有修改 Eagle/达芬奇真实数据，没有生成或提交真实凭据。
