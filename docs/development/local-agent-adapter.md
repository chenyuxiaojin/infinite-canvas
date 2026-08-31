# 本机 Agent 适配层

无限画布桌面版提供正式的本机 Agent Bridge 和 `infinite-canvas` CLI。Codex、Claude Code 等进程通过结构化协议操作画布，不需要注册网页账号，也不需要模拟鼠标点击。

## 安全边界

- Bridge 固定监听 `127.0.0.1:3102`，拒绝 `localhost`、`0.0.0.0`、IPv6 和公网地址。
- 桌面首次启动时在应用数据目录生成安装专属凭据，文件权限为 `0600`，目录权限为 `0700`。
- CLI 只从凭据文件读取认证信息；不提供 token 命令行参数，也不要把凭据写入环境变量、日志、脚本或项目导出。
- `infinite-canvas credentials revoke` 会立即废止当前 bearer，并把替代凭据原子写回同一私有文件；响应不会返回 secret。
- Bridge 没有任意 shell、任意可执行文件、任意路径、任意 URL、原始 SQL
  或付费生成入口。
- Agent 写请求必须包含 `project_id`、`request_id`、`base_revision` 和
  `actor: "agent"`。人类编辑造成 revision 变化时，Agent 写入以
  `STALE_REVISION` 失败，不覆盖较新的人工版本。
- 人工锁只来自 `CanvasProject.operationState.locks`；Agent 不能修改、删除、
  布局或连接任何锁定节点。

桌面 WebView 通过 Tauri IPC 读写同一份 SQLite `canvas_projects` 表；
Agent Bridge 通过 `CanonicalCanvasAdapter` 使用该表，并把 CLI 白名单操作
映射成公共 `CanvasOperationBatch`，交给同一个 `applyCanvasOperationBatch`
执行。不存在第二份 Agent 画布数据库、revision 或 request journal。旧的
桌面 IndexedDB 项目按 revision 优先、同 revision 再按 `updatedAt` 合并。

## 安装 CLI

桌面 `.app` 已携带 CLI。把应用安装到 `/Applications` 后，可建立一个稳定入口：

```bash
sudo ln -sf "/Applications/无限画布.app/Contents/MacOS/infinite-canvas" /usr/local/bin/infinite-canvas
```

CLI 默认连接 `http://127.0.0.1:3102`，并读取：

```text
~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/agent-bridge/credential.json
```

不要查看或复制该文件内容。需要使用开发实例时，只传不同的凭据文件路径；凭据本身仍不出现在命令行：

```bash
infinite-canvas --credential-file /path/to/private/credential.json capabilities
```

## 常用命令

所有成功和业务错误都输出 JSON。稳定退出码为：`0` 成功、`2` 参数或请求
schema 错误、`3` Bridge/运行时不可用、`4` 未认证、`5` revision/幂等冲突、
`6` 未找到、`7` 能力被策略拒绝、`1` 其他内部错误。

```bash
infinite-canvas capabilities
infinite-canvas projects list
infinite-canvas projects get PROJECT_ID
infinite-canvas canvas operations dry-run --file request.json
infinite-canvas canvas operations apply --file request.json
infinite-canvas runtime
infinite-canvas media inbox
infinite-canvas media video ingest --file video-ingest-request.json
infinite-canvas media image ingest --file image-ingest-request.json
infinite-canvas generation video request --file video-generation-request.json
infinite-canvas tasks status TASK_ID
infinite-canvas tasks cancel TASK_ID
infinite-canvas tasks test-clip --file test-clip-request.json
infinite-canvas credentials revoke
```

`--file -` 可从标准输入读取 JSON。生成类命令只有确定性的本地测试片，不调用模型、不扣费。

## 受控媒体摄入

`media inbox` 不回传绝对路径。正式桌面包的固定 inbox 按约定位于：

```text
~/Library/Application Support/com.chenyuxiaojin.infinitecanvas/agent-media/inbox
```

Agent 以同一 POSIX 用户把文件复制成该目录内的一层 basename（不建子目录），再提交
摄入请求；请求只含 basename 和小写 SHA-256，不含任何路径。视频只收 `.mp4`
（上限 1 GiB，异步 canvas task + system 回填）；图片收 `.png/.jpg/.jpeg/.webp`
（上限 100 MiB，同步验收，单个原子批次直接建成品 `image` 节点，不产生 task）。
目录穿越、符号链接、摘要不匹配、空文件、零尺寸图片均被结构化拒绝；重复提交同
request 幂等，图片在 inbox 文件清理后重放仍返回同一 `local-ref:` 引用。

```json
{
  "project_id": "PROJECT_ID",
  "node_id": "agent-image-1",
  "request_id": "agent-image-ingest-0001",
  "base_revision": 12,
  "actor": "agent",
  "inbox_file_name": "frame-001.png",
  "expected_sha256": "小写 64 位十六进制",
  "title": "S01 关键帧",
  "position": { "x": 240, "y": 160 },
  "size": { "width": 320, "height": 180 }
}
```

## 付费生成（人工批准闸门）

`generation video request` 是唯一的付费入口，且**不直接花钱**：它只在画布上创建
视频占位节点、关键帧来源连线和一个 `pending_approval` 任务，同时给出预计成本。
只有用户在画布节点卡上点「批准生成」，桌面执行器才调用 MiniMax-H3 API；点「拒绝」
任务直接终止。该闸门是协议 reducer 级不变量，任何 Agent 都不能绕过或代为批准。

```json
{
  "project_id": "PROJECT_ID",
  "node_id": "agent-gen-1",
  "request_id": "agent-gen-0001",
  "base_revision": 12,
  "actor": "agent",
  "title": "S01 生成",
  "prompt": "镜头缓推……",
  "image_node_id": "已摄入的关键帧图片节点 ID",
  "resolution": "768P",
  "duration_seconds": 6,
  "position": { "x": 640, "y": 160 },
  "size": { "width": 420, "height": 236 }
}
```

- `image_node_id` 必须指向本工程内带受控 `localMedia` 引用的 image 节点（先走
  `media image ingest`）。分辨率只收 `768P`/`2K`，时长 4-15 秒。
- 响应含 `estimated_cost_yuan` 与 `canvas_task_id`；之后用 `projects get` 轮询任务
  状态（`pending_approval → queued → running → succeeded/failed`），任务 `details`
  即台账（提示词/参数/预计成本/供应商任务 ID/输出摘要）。
- H3 凭据只存桌面私有配置 `paid-generation/config.json`；Bridge 不回显 key，也不
  接受 Agent 修改配置。

## 画布操作请求

先用 `projects get` 读取最新 revision，再准备请求文件：

```json
{
  "project_id": "PROJECT_ID",
  "request_id": "agent-run-0001",
  "base_revision": 12,
  "actor": "agent",
  "operations": [
    {
      "type": "create_text_node",
      "node_id": "agent-note-1",
      "title": "Agent 草稿",
      "content": "这是可继续人工编辑的文本节点。",
      "position": { "x": 240, "y": 160 },
      "size": { "width": 360, "height": 220 }
    }
  ]
}
```

白名单操作只有：

- `create_text_node`
- `move_node`
- `set_node_text`
- `set_project_title`
- `add_connection`
- `remove_connection`

不接受文件路径、命令、URL 或自由格式节点 JSON。先执行 `dry-run`；
确认结果后使用完全相同的 base revision 执行 `apply`。重复提交相同
`request_id` 和相同 payload 会返回同一结果并标记 `duplicate: true`；
相同 `request_id` 搭配不同 payload 会返回 `REQUEST_ID_REUSED`。

本地测试片请求同样绑定项目、request 和 revision：

```json
{
  "project_id": "PROJECT_ID",
  "request_id": "local-test-clip-0001",
  "base_revision": 12,
  "actor": "agent"
}
```

## 能力来源

`capabilities` 将以下现有入口汇总为机器可读目录：

- Go REST：账号画布、图片/音频/视频任务接口。
- Tauri IPC：桌面运行时探测、固定测试片、任务状态/取消、桌面本机画布读写。
- Rust DesktopRuntime：FFmpeg、只读外部连接器、本地声音服务探测和受限任务执行。
- Agent Bridge：凭据、项目读取、白名单操作、dry-run、revision、幂等和结构化错误。

## 总装后的公共接线

最终调用链只有一条：

```text
CLI / Bridge 白名单 JSON
  -> CanonicalCanvasAdapter
  -> POST /internal/canvas-operation
  -> applyCanvasOperationBatch
  -> 原子写回同一 canvas_projects.project_data
  -> 已打开 WebView 每 500 ms 读取同一工程 revision
```

`CanvasOperationAdapter` 仍是 Bridge 的端口抽象，但实现只剩
`CanonicalCanvasAdapter`；旧 `SqliteCanvasAdapter`、SHA revision、
`agent_operation_requests` 和重复锁/reducer 已删除。

涉及接线的文件：

- `integrations/local-agent-adapter-rust/`：Bridge、CLI、凭据、能力目录、公共 adapter 和测试。
- `desktop/src-tauri/src/agent_bridge.rs`：Tauri/Bridge 组合根。
- `desktop/src-tauri/src/runtime.rs`：DesktopRuntime 的 Agent 白名单实现。
- `web/src/app/(user)/canvas/stores/use-canvas-store.ts`：
  公共 Store、桌面 IPC 与 IndexedDB 合并。
- `web/src/app/internal/canvas-operation/route.ts`：只调用公共 reducer 的
  loopback 内部执行入口，不持有状态。
- `desktop/scripts/prepare-desktop.mjs`、
  `desktop/src-tauri/tauri.conf.json`：CLI 打包。
