import type { CanvasAgentToolResult } from "./canvas-agent-tools";
import { canUndoCanvasAgentBatch, type CanvasOperationAuditEntry, type CanvasOperationState } from "../protocol/canvas-operation-protocol";
import type { CanvasAgentChangeBatch, CanvasCollaborationState, CanvasCollaborationStatus, CanvasNodeData } from "../types";

export type CanvasAgentBatchRuntime = {
    id: string;
    summary: string;
    startedAt: string;
    auditOffset: number;
    errors: string[];
    hadConflict: boolean;
};

function nowIso(now?: string) {
    return now || new Date().toISOString();
}

function createStatus(now?: string): CanvasCollaborationStatus {
    return { state: "idle", message: "Agent 待命", affectedNodeIds: [], updatedAt: nowIso(now) };
}

function statusFromOperationState(operationState: CanvasOperationState | undefined, now?: string): CanvasCollaborationStatus {
    const latest = operationState?.audit.at(-1);
    if (!latest) return createStatus(now);
    const state = latest.result.ok ? "success" : isConflict(latest) ? "conflict" : "error";
    return {
        state,
        message: latest.result.ok ? "最近一次画布操作已完成" : latest.result.error?.message || "最近一次画布操作失败",
        batchId: latest.batch.requestId,
        affectedNodeIds: affectedNodeIds(latest),
        updatedAt: latest.result.processedAt,
    };
}

function normalizeStatus(status: CanvasCollaborationStatus | null | undefined, operationState: CanvasOperationState | undefined, now?: string): CanvasCollaborationStatus {
    if (!status) return statusFromOperationState(operationState, now);
    if (status.state !== "running") return status;
    return {
        state: "error",
        message: "上次 Agent 执行在刷新前中断，请检查画布后重试",
        batchId: status.batchId,
        affectedNodeIds: status.affectedNodeIds || [],
        updatedAt: nowIso(now),
    };
}

function beginBatch(input: { status: CanvasCollaborationStatus; operationState: CanvasOperationState; batchId: string; summary: string; now?: string }) {
    const startedAt = nowIso(input.now);
    return {
        runtime: {
            id: input.batchId,
            summary: input.summary.trim().slice(0, 160) || "Agent 修改画布",
            startedAt,
            auditOffset: input.operationState.audit.length,
            errors: [],
            hadConflict: false,
        } satisfies CanvasAgentBatchRuntime,
        status: {
            state: "running",
            message: "Agent 正在读取并修改当前画布",
            batchId: input.batchId,
            affectedNodeIds: [],
            updatedAt: startedAt,
        } satisfies CanvasCollaborationStatus,
    };
}

function updateStatus(status: CanvasCollaborationStatus, message: string, now?: string): CanvasCollaborationStatus {
    if (status.state !== "running") return status;
    return { ...status, message, updatedAt: nowIso(now) };
}

function noteActionResult(input: { status: CanvasCollaborationStatus; runtime: CanvasAgentBatchRuntime; result: CanvasAgentToolResult; now?: string }) {
    if (input.result.ok) return { status: input.status, runtime: input.runtime };
    const message = typeof input.result.message === "string" ? input.result.message : "Agent 操作失败";
    if (!input.runtime.errors.includes(message)) input.runtime.errors.push(message);
    const conflict = input.result.code === "stale_revision" || input.result.code === "revision_conflict" || input.result.code === "locked_node";
    input.runtime.hadConflict ||= conflict;
    return {
        runtime: input.runtime,
        status: {
            state: conflict ? "conflict" : "error",
            message,
            batchId: input.runtime.id,
            affectedNodeIds: input.status.affectedNodeIds,
            updatedAt: nowIso(input.now),
        } satisfies CanvasCollaborationStatus,
    };
}

function finishBatch(input: { status: CanvasCollaborationStatus; runtime: CanvasAgentBatchRuntime; operationState: CanvasOperationState; fatalError?: string; now?: string }): CanvasCollaborationStatus {
    const entries = input.operationState.audit.slice(input.runtime.auditOffset).filter((entry) => entry.batch.actor === "agent");
    const errors = [...input.runtime.errors];
    if (input.fatalError && !errors.includes(input.fatalError)) errors.push(input.fatalError);
    const conflict = input.runtime.hadConflict || entries.some(isConflict);
    const failed = Boolean(errors.length || entries.some((entry) => !entry.result.ok));
    const affected = Array.from(new Set(entries.flatMap(affectedNodeIds)));
    return {
        state: conflict ? "conflict" : failed ? "error" : "success",
        message: conflict ? errors.at(-1) || entries.findLast((entry) => isConflict(entry))?.result.error?.message || "Agent 修改遇到冲突" : failed ? errors.at(-1) || entries.findLast((entry) => !entry.result.ok)?.result.error?.message || "Agent 执行失败" : affected.length ? `Agent 已完成，影响 ${affected.length} 个节点` : "Agent 已完成，本轮没有修改画布",
        batchId: input.runtime.id,
        affectedNodeIds: affected,
        updatedAt: nowIso(input.now),
    };
}

function toView(operationState: CanvasOperationState, status: CanvasCollaborationStatus, nodes: CanvasNodeData[]): CanvasCollaborationState {
    const titleById = new Map(nodes.map((node) => [node.id, node.title || "未命名节点"]));
    const undoneAtByRequest = new Map(
        operationState.audit.flatMap((entry) => entry.undoneByRequestId ? [[entry.batch.requestId, operationState.requests[entry.undoneByRequestId]?.result.processedAt || entry.result.processedAt] as const] : []),
    );
    const batches = operationState.audit
        .filter((entry) => entry.batch.actor === "agent")
        .map((entry): CanvasAgentChangeBatch => {
            const nodeIds = affectedNodeIds(entry);
            return {
                id: entry.batch.requestId,
                actor: "Canvas Agent",
                startedAt: entry.batch.timestamp,
                completedAt: entry.result.processedAt,
                summary: operationSummary(entry),
                actionNames: entry.batch.operations.map((operation) => operation.type),
                affectedNodeIds: nodeIds,
                affectedNodeTitles: Object.fromEntries(nodeIds.map((id) => [id, titleById.get(id) || deletedNodeTitle(entry, id) || id])),
                baseRevision: entry.result.baseRevision,
                revision: entry.result.revision,
                status: entry.result.ok ? "success" : isConflict(entry) ? "conflict" : "error",
                reversible: Boolean(entry.result.ok && entry.undoSnapshot),
                canUndoNow: canUndoCanvasAgentBatch(operationState, entry.batch.requestId),
                error: entry.result.error?.message,
                undoneAt: undoneAtByRequest.get(entry.batch.requestId),
            };
        });
    return { revision: operationState.revision, batches, status };
}

function latestUndoTarget(operationState: CanvasOperationState) {
    return operationState.audit.findLast((entry) => entry.batch.actor === "agent" && entry.result.ok && entry.undoSnapshot && !entry.undoneByRequestId);
}

function lastAgentChangedAt(operationState: CanvasOperationState, nodeId: string) {
    return operationState.audit.findLast((entry) => entry.batch.actor === "agent" && entry.result.ok && affectedNodeIds(entry).includes(nodeId))?.result.processedAt;
}

function nodeCollaboration(operationState: CanvasOperationState, nodeId: string) {
    const lastEdit = operationState.audit.findLast((entry) => entry.result.ok && affectedNodeIds(entry).includes(nodeId));
    return {
        lockedByHuman: Boolean(operationState.locks[nodeId]),
        revision: lastEdit?.result.revision || 0,
        lastEditedBy: lastEdit?.batch.actor,
        lastAgentChangedAt: lastAgentChangedAt(operationState, nodeId),
    };
}

function isConflict(entry: CanvasOperationAuditEntry) {
    return entry.result.error?.code === "stale_revision" || entry.result.error?.code === "locked_node";
}

function affectedNodeIds(entry: CanvasOperationAuditEntry) {
    const ids = new Set<string>();
    entry.result.operationResults.forEach((result) => {
        if (result.nodeId) ids.add(result.nodeId);
        result.nodeIds?.forEach((id) => ids.add(id));
    });
    if (entry.result.error?.nodeId) ids.add(entry.result.error.nodeId);
    entry.batch.operations.forEach((operation) => {
        if (operation.type === "node.create") ids.add(operation.node.id);
        if (operation.type === "node.update" || operation.type === "node.delete" || operation.type === "lock.set") ids.add(operation.nodeId);
        if (operation.type === "connection.create") {
            ids.add(operation.connection.fromNodeId);
            ids.add(operation.connection.toNodeId);
        }
        if (operation.type === "layout.apply") Object.keys(operation.positions).forEach((id) => ids.add(id));
        if (operation.type === "task.start") ids.add(operation.task.nodeId);
    });
    return Array.from(ids);
}

function deletedNodeTitle(entry: CanvasOperationAuditEntry, nodeId: string) {
    return entry.undoSnapshot?.nodes.find((node) => node.id === nodeId)?.title;
}

function operationSummary(entry: CanvasOperationAuditEntry) {
    const names = Array.from(new Set(entry.batch.operations.map((operation) => operation.type)));
    return names.length ? names.join("、") : "Agent 修改画布";
}

export const canvasCollaborationAdapter = {
    createStatus,
    normalizeStatus,
    beginBatch,
    updateStatus,
    noteActionResult,
    finishBatch,
    toView,
    latestUndoTarget,
    lastAgentChangedAt,
    nodeCollaboration,
};
