import type { CanvasAgentBatchUndo, CanvasAgentChangeBatch, CanvasCollaborationState, CanvasConnection, CanvasNodeData, CanvasNodeMetadata, CanvasNodeUndoPatch } from "../types";

const MAX_AGENT_BATCHES = 20;
const IRREVERSIBLE_ACTIONS = new Set(["generate_image", "edit_image", "generate_video", "generate_audio", "delete_node"]);

export type CanvasAgentBatchRuntime = {
    id: string;
    summary: string;
    startedAt: string;
    baseRevision: number;
    expectedRevision: number;
    beforeNodes: CanvasNodeData[];
    beforeConnections: CanvasConnection[];
    actionNames: Set<string>;
    affectedNodeIds: Set<string>;
    errors: string[];
    conflicted: boolean;
    hadConflict: boolean;
    reversible: boolean;
};

export type CanvasMutationGuard = { ok: true } | { ok: false; code: "revision_conflict" | "locked_node"; message: string };

export type CanvasUndoResult = { ok: true; state: CanvasCollaborationState; nodes: CanvasNodeData[]; connections: CanvasConnection[]; batchId: string } | { ok: false; state: CanvasCollaborationState; reason: string };

export interface CanvasCollaborationAdapter {
    createState(now?: string): CanvasCollaborationState;
    normalize(state: CanvasCollaborationState | null | undefined, now?: string): CanvasCollaborationState;
    beginBatch(input: { state: CanvasCollaborationState; batchId: string; summary: string; nodes: CanvasNodeData[]; connections: CanvasConnection[]; now?: string }): { state: CanvasCollaborationState; runtime: CanvasAgentBatchRuntime };
    guardMutation(state: CanvasCollaborationState, runtime: CanvasAgentBatchRuntime, lockedNodeIds?: string[]): CanvasMutationGuard;
    recordAction(input: {
        state: CanvasCollaborationState;
        runtime: CanvasAgentBatchRuntime;
        actionName: string;
        beforeNodes: CanvasNodeData[];
        beforeConnections: CanvasConnection[];
        afterNodes: CanvasNodeData[];
        afterConnections: CanvasConnection[];
        result: { ok: boolean; code?: string; message?: string };
        now?: string;
    }): { state: CanvasCollaborationState; runtime: CanvasAgentBatchRuntime; nodes: CanvasNodeData[] };
    finishBatch(input: { state: CanvasCollaborationState; runtime: CanvasAgentBatchRuntime; nodes: CanvasNodeData[]; connections: CanvasConnection[]; now?: string; fatalError?: string }): {
        state: CanvasCollaborationState;
        batch?: CanvasAgentChangeBatch;
    };
    updateStatus(state: CanvasCollaborationState, message: string, now?: string): CanvasCollaborationState;
    markHumanNodes(state: CanvasCollaborationState, nodes: CanvasNodeData[], nodeIds: Iterable<string>, now?: string): { state: CanvasCollaborationState; nodes: CanvasNodeData[] };
    undoLatest(state: CanvasCollaborationState, nodes: CanvasNodeData[], connections: CanvasConnection[], now?: string): CanvasUndoResult;
}

function nowIso(now?: string) {
    return now || new Date().toISOString();
}

function createState(now?: string): CanvasCollaborationState {
    const at = nowIso(now);
    return {
        revision: 0,
        batches: [],
        status: { state: "idle", message: "Agent 待命", affectedNodeIds: [], updatedAt: at },
    };
}

function normalize(state: CanvasCollaborationState | null | undefined, now?: string): CanvasCollaborationState {
    if (!state) return createState(now);
    const interrupted = state.status?.state === "running";
    return {
        revision: Number.isFinite(state.revision) ? Math.max(0, state.revision) : 0,
        batches: Array.isArray(state.batches) ? state.batches.slice(-MAX_AGENT_BATCHES) : [],
        status: interrupted
            ? {
                  state: "error",
                  message: "上次 Agent 执行在刷新前中断，请检查画布后重试",
                  batchId: state.status.batchId,
                  affectedNodeIds: state.status.affectedNodeIds || [],
                  updatedAt: nowIso(now),
              }
            : state.status || createState(now).status,
    };
}

function beginBatch({ state, batchId, summary, nodes, connections, now }: Parameters<CanvasCollaborationAdapter["beginBatch"]>[0]) {
    const startedAt = nowIso(now);
    const runtime: CanvasAgentBatchRuntime = {
        id: batchId,
        summary: summary.trim().slice(0, 160) || "Agent 修改画布",
        startedAt,
        baseRevision: state.revision,
        expectedRevision: state.revision,
        beforeNodes: nodes,
        beforeConnections: connections,
        actionNames: new Set(),
        affectedNodeIds: new Set(),
        errors: [],
        conflicted: false,
        hadConflict: false,
        reversible: true,
    };
    return {
        runtime,
        state: {
            ...state,
            status: {
                state: "running" as const,
                message: "Agent 正在读取并修改当前画布",
                batchId,
                affectedNodeIds: [],
                updatedAt: startedAt,
            },
        },
    };
}

function guardMutation(state: CanvasCollaborationState, runtime: CanvasAgentBatchRuntime, lockedNodeIds: string[] = []): CanvasMutationGuard {
    if (runtime.conflicted || state.revision !== runtime.expectedRevision) {
        runtime.conflicted = true;
        runtime.hadConflict = true;
        return {
            ok: false,
            code: "revision_conflict",
            message: `画布已从 revision ${runtime.expectedRevision} 更新到 ${state.revision}，Agent 已停止覆盖，请检查后重试`,
        };
    }
    if (lockedNodeIds.length) {
        return {
            ok: false,
            code: "locked_node",
            message: `节点 ${lockedNodeIds.join("、")} 已被人工锁定，Agent 未修改`,
        };
    }
    return { ok: true };
}

function changedNodeIds(before: CanvasNodeData[], after: CanvasNodeData[]) {
    const beforeById = new Map(before.map((node) => [node.id, node]));
    const afterById = new Map(after.map((node) => [node.id, node]));
    const changed = new Set<string>();
    beforeById.forEach((node, id) => {
        if (!afterById.has(id) || afterById.get(id) !== node) changed.add(id);
    });
    afterById.forEach((node, id) => {
        if (!beforeById.has(id) || beforeById.get(id) !== node) changed.add(id);
    });
    return changed;
}

function changedConnectionNodeIds(before: CanvasConnection[], after: CanvasConnection[]) {
    const beforeById = new Map(before.map((connection) => [connection.id, connection]));
    const afterById = new Map(after.map((connection) => [connection.id, connection]));
    const nodeIds = new Set<string>();
    beforeById.forEach((connection, id) => {
        if (afterById.get(id) === connection) return;
        nodeIds.add(connection.fromNodeId);
        nodeIds.add(connection.toNodeId);
    });
    afterById.forEach((connection, id) => {
        if (beforeById.get(id) === connection) return;
        nodeIds.add(connection.fromNodeId);
        nodeIds.add(connection.toNodeId);
    });
    return nodeIds;
}

function recordAction({ state, runtime, actionName, beforeNodes, beforeConnections, afterNodes, afterConnections, result, now }: Parameters<CanvasCollaborationAdapter["recordAction"]>[0]) {
    const at = nowIso(now);
    runtime.actionNames.add(actionName);
    if (IRREVERSIBLE_ACTIONS.has(actionName)) runtime.reversible = false;

    if (!result.ok) {
        const message = result.message || "Agent 操作失败";
        runtime.errors.push(message);
        if (result.code === "revision_conflict") runtime.conflicted = true;
        if (result.code === "revision_conflict" || result.code === "locked_node") runtime.hadConflict = true;
        return {
            runtime,
            nodes: afterNodes,
            state: {
                ...state,
                status: {
                    state: result.code === "revision_conflict" || result.code === "locked_node" ? ("conflict" as const) : ("error" as const),
                    message,
                    batchId: runtime.id,
                    affectedNodeIds: Array.from(runtime.affectedNodeIds),
                    updatedAt: at,
                },
            },
        };
    }

    const interleavedHumanChange = state.revision !== runtime.expectedRevision;
    if (interleavedHumanChange) {
        runtime.conflicted = true;
        runtime.hadConflict = true;
        runtime.errors.push(`Agent 操作期间画布 revision 已从 ${runtime.expectedRevision} 更新到 ${state.revision}，请检查本次结果`);
    }
    const nodeIds = changedNodeIds(beforeNodes, afterNodes);
    changedConnectionNodeIds(beforeConnections, afterConnections).forEach((id) => nodeIds.add(id));
    if (!nodeIds.size && beforeConnections === afterConnections && beforeNodes === afterNodes) {
        return { state, runtime, nodes: afterNodes };
    }

    nodeIds.forEach((id) => runtime.affectedNodeIds.add(id));
    const revision = state.revision + 1;
    runtime.expectedRevision = revision;
    const nodes = afterNodes.map((node) =>
        nodeIds.has(node.id)
            ? {
                  ...node,
                  collaboration: {
                      ...node.collaboration,
                      locked: node.collaboration?.locked,
                      revision,
                      lastEditedBy: "agent" as const,
                      lastAgentChangedAt: at,
                      lastAgentBatchId: runtime.id,
                  },
              }
            : node,
    );
    return {
        runtime,
        nodes,
        state: {
            ...state,
            revision,
            status: {
                state: interleavedHumanChange ? ("conflict" as const) : ("running" as const),
                message: interleavedHumanChange ? runtime.errors.at(-1)! : "Agent 正在执行 " + actionName,
                batchId: runtime.id,
                affectedNodeIds: Array.from(runtime.affectedNodeIds),
                updatedAt: at,
            },
        },
    };
}

function updateStatus(state: CanvasCollaborationState, message: string, now?: string): CanvasCollaborationState {
    if (state.status.state !== "running") return state;
    return { ...state, status: { ...state.status, message, updatedAt: nowIso(now) } };
}

function finishBatch({ state, runtime, nodes, connections, now, fatalError }: Parameters<CanvasCollaborationAdapter["finishBatch"]>[0]) {
    const completedAt = nowIso(now);
    if (fatalError) runtime.errors.push(fatalError);
    const undo = buildUndo(runtime.beforeNodes, runtime.beforeConnections, nodes, connections);
    const hasChanges = runtime.affectedNodeIds.size > 0 || undo.removeConnectionIds.length > 0 || undo.restoreConnections.length > 0;
    const status = runtime.hadConflict ? "conflict" : runtime.errors.length || fatalError ? "error" : "success";
    const reversible = hasChanges && runtime.reversible && status !== "conflict";
    const shouldRecord = hasChanges || status !== "success";
    const nodeTitleById = new Map([...runtime.beforeNodes, ...nodes].map((node) => [node.id, node.title || "未命名节点"]));
    const batch: CanvasAgentChangeBatch | undefined = shouldRecord
        ? {
              id: runtime.id,
              actor: "Canvas Agent",
              startedAt: runtime.startedAt,
              completedAt,
              summary: runtime.summary,
              actionNames: Array.from(runtime.actionNames),
              affectedNodeIds: Array.from(runtime.affectedNodeIds),
              affectedNodeTitles: Object.fromEntries(Array.from(runtime.affectedNodeIds).map((id) => [id, nodeTitleById.get(id) || id])),
              baseRevision: runtime.baseRevision,
              revision: state.revision,
              status,
              reversible,
              error: runtime.errors.at(-1),
              ...(reversible ? { undo } : {}),
          }
        : undefined;
    const nextState: CanvasCollaborationState = {
        ...state,
        batches: batch ? [...state.batches, batch].slice(-MAX_AGENT_BATCHES) : state.batches,
        status: {
            state: status,
            message:
                status === "conflict"
                    ? runtime.errors.at(-1) || "Agent 修改遇到 revision 或锁定冲突"
                    : status === "error"
                      ? runtime.errors.at(-1) || "Agent 执行失败"
                      : hasChanges
                        ? `Agent 已完成，影响 ${runtime.affectedNodeIds.size} 个节点`
                        : "Agent 已完成，本轮没有修改画布",
            batchId: runtime.id,
            affectedNodeIds: Array.from(runtime.affectedNodeIds),
            updatedAt: completedAt,
        },
    };
    return { state: nextState, batch };
}

function markHumanNodes(state: CanvasCollaborationState, nodes: CanvasNodeData[], nodeIds: Iterable<string>, now?: string) {
    const changedIds = new Set(nodeIds);
    if (!changedIds.size) return { state, nodes };
    const revision = state.revision + 1;
    const at = nowIso(now);
    return {
        state: {
            ...state,
            revision,
            status:
                state.status.state === "running"
                    ? {
                          state: "conflict" as const,
                          message: `人工已在 Agent 执行期间更新画布（revision ${revision}），后续 Agent 覆盖已暂停`,
                          batchId: state.status.batchId,
                          affectedNodeIds: Array.from(new Set([...state.status.affectedNodeIds, ...changedIds])),
                          updatedAt: at,
                      }
                    : state.status,
        },
        nodes: nodes.map((node) =>
            changedIds.has(node.id)
                ? {
                      ...node,
                      collaboration: {
                          ...node.collaboration,
                          locked: node.collaboration?.locked,
                          revision,
                          lastEditedBy: "human" as const,
                          lastHumanChangedAt: at,
                      },
                  }
                : node,
        ),
    };
}

function buildUndo(beforeNodes: CanvasNodeData[], beforeConnections: CanvasConnection[], afterNodes: CanvasNodeData[], afterConnections: CanvasConnection[]): CanvasAgentBatchUndo {
    const beforeNodeById = new Map(beforeNodes.map((node, index) => [node.id, { node, index }]));
    const afterNodeById = new Map(afterNodes.map((node) => [node.id, node]));
    const removeNodeIds = afterNodes.filter((node) => !beforeNodeById.has(node.id)).map((node) => node.id);
    const restoreNodes: CanvasNodeUndoPatch[] = [];

    beforeNodeById.forEach(({ node: before, index }, id) => {
        const after = afterNodeById.get(id);
        if (!after) {
            restoreNodes.push({ id, index, full: before });
            return;
        }
        if (after === before) return;
        const fields: CanvasNodeUndoPatch["fields"] = {};
        if (after.type !== before.type) fields.type = before.type;
        if (after.title !== before.title) fields.title = before.title;
        if (!sameValue(after.position, before.position)) fields.position = before.position;
        if (after.width !== before.width) fields.width = before.width;
        if (after.height !== before.height) fields.height = before.height;
        if (!sameValue(after.collaboration, before.collaboration)) fields.collaboration = before.collaboration;
        const metadata: Partial<CanvasNodeMetadata> = {};
        const removeMetadataKeys: Array<keyof CanvasNodeMetadata> = [];
        const metadataKeys = new Set([...Object.keys(before.metadata || {}), ...Object.keys(after.metadata || {})] as Array<keyof CanvasNodeMetadata>);
        metadataKeys.forEach((key) => {
            const beforeHasKey = Object.prototype.hasOwnProperty.call(before.metadata || {}, key);
            const afterHasKey = Object.prototype.hasOwnProperty.call(after.metadata || {}, key);
            if (beforeHasKey === afterHasKey && sameValue(before.metadata?.[key], after.metadata?.[key])) return;
            if (!beforeHasKey) removeMetadataKeys.push(key);
            else Object.assign(metadata, { [key]: before.metadata?.[key] });
        });
        restoreNodes.push({
            id,
            ...(Object.keys(fields).length ? { fields } : {}),
            ...(Object.keys(metadata).length ? { metadata } : {}),
            ...(removeMetadataKeys.length ? { removeMetadataKeys } : {}),
        });
    });

    const beforeConnectionById = new Map(beforeConnections.map((connection, index) => [connection.id, { connection, index }]));
    const afterConnectionById = new Map(afterConnections.map((connection) => [connection.id, connection]));
    return {
        removeNodeIds,
        restoreNodes,
        removeConnectionIds: afterConnections.filter((connection) => !beforeConnectionById.has(connection.id)).map((connection) => connection.id),
        restoreConnections: beforeConnections.flatMap((connection, index) => (afterConnectionById.has(connection.id) ? [] : [{ connection, index }])),
    };
}

function sameValue(left: unknown, right: unknown) {
    if (Object.is(left, right)) return true;
    if (!left || !right || typeof left !== "object" || typeof right !== "object") return false;
    try {
        return JSON.stringify(left) === JSON.stringify(right);
    } catch {
        return false;
    }
}

function undoLatest(state: CanvasCollaborationState, nodes: CanvasNodeData[], connections: CanvasConnection[], now?: string): CanvasUndoResult {
    const batchIndex = state.batches.findLastIndex((batch) => batch.reversible && batch.undo && !batch.undoneAt);
    if (batchIndex < 0) return { ok: false, state, reason: "没有可撤销的 Agent 批次" };
    const batch = state.batches[batchIndex];
    if (state.revision !== batch.revision) {
        return { ok: false, state, reason: `画布已从批次 revision ${batch.revision} 更新到 ${state.revision}，为避免覆盖人工修改，不能直接撤销` };
    }
    const undo = batch.undo!;
    const removeNodeIds = new Set(undo.removeNodeIds);
    let nextNodes = nodes.filter((node) => !removeNodeIds.has(node.id));
    const restoreById = new Map(undo.restoreNodes.map((patch) => [patch.id, patch]));
    nextNodes = nextNodes.map((node) => {
        const patch = restoreById.get(node.id);
        if (!patch || patch.full) return node;
        const metadata = { ...node.metadata, ...patch.metadata };
        patch.removeMetadataKeys?.forEach((key) => delete metadata[key]);
        return {
            ...node,
            ...patch.fields,
            ...(patch.metadata || patch.removeMetadataKeys?.length ? { metadata } : {}),
        };
    });
    undo.restoreNodes
        .filter((patch): patch is CanvasNodeUndoPatch & { full: CanvasNodeData } => Boolean(patch.full))
        .sort((left, right) => (left.index || 0) - (right.index || 0))
        .forEach((patch) => nextNodes.splice(Math.min(patch.index ?? nextNodes.length, nextNodes.length), 0, patch.full));

    const removeConnectionIds = new Set(undo.removeConnectionIds);
    let nextConnections = connections.filter((connection) => !removeConnectionIds.has(connection.id));
    undo.restoreConnections
        .slice()
        .sort((left, right) => left.index - right.index)
        .forEach(({ connection, index }) => {
            if (nextConnections.some((item) => item.id === connection.id)) return;
            nextConnections.splice(Math.min(index, nextConnections.length), 0, connection);
        });
    const revision = state.revision + 1;
    const at = nowIso(now);
    const batches = state.batches.map((item, index) => (index === batchIndex ? { ...item, undoneAt: at, undoneRevision: revision } : item));
    return {
        ok: true,
        batchId: batch.id,
        nodes: nextNodes,
        connections: nextConnections,
        state: {
            ...state,
            revision,
            batches,
            status: {
                state: "success",
                message: `已撤销 Agent 批次：${batch.summary}`,
                batchId: batch.id,
                affectedNodeIds: batch.affectedNodeIds,
                updatedAt: at,
            },
        },
    };
}

export const canvasCollaborationAdapter: CanvasCollaborationAdapter = {
    createState,
    normalize,
    beginBatch,
    guardMutation,
    recordAction,
    finishBatch,
    updateStatus,
    markHumanNodes,
    undoLatest,
};
