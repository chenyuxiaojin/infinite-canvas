---
title: 人与 Agent 共用画布操作协议 Handoff
description: 本机 Agent Bridge 和画布 UI 共用的工程修改接口、冲突语义与接线示例
---

# 人与 Agent 共用画布操作协议 Handoff

## 单一入口

核心协议位于：

```text
web/src/app/(user)/canvas/protocol/canvas-operation-protocol.ts
```

对外使用两层接口：

- 纯函数 `applyCanvasOperationBatch(project, batch)`：输入当前工程和批次，返回新工程与结构化回执。用于测试、导入迁移或任何不依赖 React 的调用者。
- Zustand `useCanvasStore.getState().applyOperationBatch(batch)`：对当前唯一画布工程执行同一纯函数，随后写回 localForage，并在账号同步开启时沿用原有云端保存。

本机 Agent Bridge 不应维护第二份节点/连线状态。Bridge 收到请求后，应将 `CanvasOperationBatch` 交给网页运行时的 store 入口，或者读取同一份已持久化 `CanvasProject` 后调用纯函数并原子写回。不要在 Rust、Bridge 或侧车文件中镜像一份画布。

## 批次包络

```ts
type CanvasOperationBatch = {
  protocolVersion: 1;
  actor: "human" | "agent" | "system";
  requestId: string;
  projectId: string;
  baseRevision: number;
  timestamp: string;
  operations: CanvasOperation[];
};
```

必须使用读到的当前 `project.operationState.revision` 作为 `baseRevision`。不要在 Bridge 内自增 revision。

已支持的语义操作：

- `project.update`
- `node.create` / `node.update` / `node.delete`
- `connection.create` / `connection.delete`
- `layout.apply`
- `task.start` / `task.cancel` / `task.update`（执行器回填，仅 `system` 可用）
- `lock.set`（仅 `human` 可用）
- `batch.undo`（仅 `human` / `system` 可撤销成功的 Agent 批次）

批次是原子的：任一操作失败，节点、连线、锁和任务都不会留下部分改动。失败回执仍会进入审计和幂等索引，所以调用者必须保存返回的 `project`，不能只在 `result.ok === true` 时保存。

## Agent Bridge 接线

```ts
import { CANVAS_OPERATION_PROTOCOL_VERSION } from "@/app/(user)/canvas/protocol/canvas-operation-protocol";
import { useCanvasStore } from "@/app/(user)/canvas/stores/use-canvas-store";

const store = useCanvasStore.getState();
const project = store.openProject(projectId);
if (!project) throw new Error("project_not_found");

const outcome = store.applyOperationBatch({
  protocolVersion: CANVAS_OPERATION_PROTOCOL_VERSION,
  actor: "agent",
  requestId: bridgeRequestId,
  projectId,
  baseRevision: project.operationState.revision,
  timestamp: new Date().toISOString(),
  operations: [
    { type: "node.create", node: generatedNode },
    {
      type: "connection.create",
      connection: {
        id: generatedConnectionId,
        fromNodeId: sourceNodeId,
        toNodeId: generatedNode.id,
      },
    },
  ],
});

if (!outcome) throw new Error("project_not_found");
return outcome.result;
```

Bridge 请求的 `requestId`、新节点 ID、新连线 ID 必须在重试时保持不变。同一语义批次重放会返回 `duplicate: true`，不会再次创建节点或发起任务。同一 `requestId` 改用其他操作会返回 `request_id_reused`。

## UI 接线

现有 UI 继续调用 `updateProject(projectId, { nodes, connections, ... })`。Store 会将新旧节点/连线差异转成 `actor: "human"` 的同一批次协议，所以无需大规模重写现有组件。

锁定按钮可直接提交：

```ts
const project = useCanvasStore.getState().openProject(projectId);
if (!project) return;

useCanvasStore.getState().applyOperationBatch({
  protocolVersion: 1,
  actor: "human",
  requestId: crypto.randomUUID(),
  projectId,
  baseRevision: project.operationState.revision,
  timestamp: new Date().toISOString(),
  operations: [{ type: "lock.set", nodeId, locked: true }],
});
```

解锁传 `locked: false`。UI 判断锁定状态时读取 `project.operationState.locks[nodeId]`，不要另建组件本地锁集合。

## 任务与取消

`task.start` 记录一次可审计的任务发起意图，初始状态为 `queued` 或 `running`；`task.cancel` 将其改为 `cancel_requested`。它们不自行调用付费模型或执行器。Agent Bridge 分支应按以下顺序接线：

1. 提交 `task.start`，并保存成功回执。
2. 只在 `result.ok === true && result.duplicate === false` 时调用真实执行器。
3. 执行器回执使用 `actor: "system"` 的新 request，通过 `task.update` 回填 `running` / `cancelled` / `succeeded` / `failed`；需要时在同批中用 `node.update` 回填节点展示状态。
4. 收到 `cancel_requested` 后调用白名单取消能力；不要把任意 shell 参数从批次透传给执行器。

## 冲突与人工优先

- `stale_revision`：`baseRevision` 不是当前 revision。调用者应重读项目，重新评估意图，不要盲目改 revision 后重放。
- `locked_node`：Agent 修改、删除、布局、连线或任务操作触及人工锁定节点。
- `lock_forbidden`：Agent 或 system 尝试锁定/解锁。
- `request_id_reused`：同一 request ID 被用于不同操作。
- `undo_forbidden`：撤销不是人工/system 发起，或 Agent 批次后已有新的结构修改。后一条用于避免快照覆盖后续人工结果。

画布 revision 只在成功结构批次后增加。失败批次会入审计，但不增加 revision。

## 撤销与审计

每个成功 Agent 批次的 `operationState.audit` 条目都带 `undoSnapshot`。撤销示例：

```ts
const project = useCanvasStore.getState().openProject(projectId);
if (!project) return;

useCanvasStore.getState().applyOperationBatch({
  protocolVersion: 1,
  actor: "human",
  requestId: crypto.randomUUID(),
  projectId,
  baseRevision: project.operationState.revision,
  timestamp: new Date().toISOString(),
  operations: [{ type: "batch.undo", targetRequestId: agentRequestId }],
});
```

直接恢复只允许目标 Agent 批次仍是最新结构 revision。否则保留快照作为等价 undo 信息，由 UI 做差异预览/人工确认，不能自动覆盖后续人工修改。

## 迁移与导入导出

- 旧工程没有 `operationState` 时，`migrateCanvasProject` 原样保留节点、连线和其他字段，初始 revision 为 `0`。
- 旧节点 metadata 中的 `imageTaskId` / `videoTaskId` / `audioTaskId` / `localTaskId` 会迁移成协议任务索引，原 metadata 不删除。
- Zustand 本地补水、远程列表/同步、新建和导入都调用迁移。
- ZIP 仍使用现有 export version `3`，`project` 对象会原样带上 `operationState`；旧 version `3` 文件仍可导入。
- 导入副本生成新 project ID 后，`rebindCanvasProjectIdentity` 会同步重绑定
  审计批次、回执和 request 指纹；结构、锁、任务、历史与幂等语义不丢失。
- 带 `storageKey` 的本地媒体不会把恢复时生成的临时 `blob:` / `data:` URL
  当成画布修改；持久化引用统一为稳定 storage key。
- Go 后端将整个 project JSON 作为 `ProjectData` 保存，无需数据库表迁移。

## 自动化证据

```bash
cd web
bun run test:canvas-protocol
```

契约测试覆盖：人和 Agent 共用 reducer、重复 request、锁定、过期 revision、原子冲突、安全撤销、任务发起/取消、媒体恢复不制造 revision、导入身份重绑定、旧工程迁移与 JSON 重载一致。
