import type { CanvasConnection, CanvasNodeData, CanvasNodeMetadata, Position } from "../types";

export const CANVAS_OPERATION_PROTOCOL_VERSION = 1 as const;

export type CanvasOperationActor = "human" | "agent" | "system";

export type CanvasProtocolTaskStatus = "queued" | "running" | "cancel_requested" | "cancelled" | "succeeded" | "failed";

export type CanvasProtocolTask = {
    id: string;
    nodeId: string;
    kind: string;
    status: CanvasProtocolTaskStatus;
    createdAt: string;
    updatedAt: string;
    requestId?: string;
    details?: Record<string, unknown>;
};

export type CanvasNodeLock = {
    nodeId: string;
    lockedAt: string;
    requestId: string;
    actor: "human";
};

export type CanvasNodeUpdatePatch = Partial<Omit<CanvasNodeData, "id">>;

export type CanvasOperation =
    | { type: "project.update"; title: string }
    | { type: "node.create"; node: CanvasNodeData }
    | { type: "node.update"; nodeId: string; patch: CanvasNodeUpdatePatch }
    | { type: "node.delete"; nodeId: string }
    | { type: "connection.create"; connection: CanvasConnection }
    | { type: "connection.delete"; connectionId: string }
    | { type: "layout.apply"; positions: Record<string, Position> }
    | { type: "task.start"; task: Omit<CanvasProtocolTask, "status" | "createdAt" | "updatedAt"> & { status?: "queued" | "running" } }
    | { type: "task.cancel"; taskId: string; reason?: string }
    | { type: "task.update"; taskId: string; status: Exclude<CanvasProtocolTaskStatus, "queued" | "cancel_requested">; details?: Record<string, unknown> }
    | { type: "lock.set"; nodeId: string; locked: boolean }
    | { type: "batch.undo"; targetRequestId: string };

export type CanvasOperationBatch = {
    protocolVersion: typeof CANVAS_OPERATION_PROTOCOL_VERSION;
    actor: CanvasOperationActor;
    requestId: string;
    projectId: string;
    baseRevision: number;
    timestamp: string;
    operations: CanvasOperation[];
};

export type CanvasOperationError = {
    code:
        | "invalid_batch"
        | "project_mismatch"
        | "request_id_reused"
        | "stale_revision"
        | "node_not_found"
        | "node_exists"
        | "invalid_node"
        | "locked_node"
        | "connection_not_found"
        | "connection_exists"
        | "invalid_connection"
        | "task_not_found"
        | "task_exists"
        | "task_terminal"
        | "task_update_forbidden"
        | "lock_forbidden"
        | "undo_not_found"
        | "undo_forbidden"
        | "already_undone";
    message: string;
    operationIndex?: number;
    nodeId?: string;
    currentRevision?: number;
};

export type CanvasOperationResultItem = {
    type: CanvasOperation["type"];
    nodeId?: string;
    nodeIds?: string[];
    connectionId?: string;
    connectionIds?: string[];
    taskId?: string;
    targetRequestId?: string;
    alreadyExists?: boolean;
    locked?: boolean;
    title?: string;
};

export type CanvasOperationBatchResult = {
    ok: boolean;
    status: "applied" | "rejected";
    duplicate: boolean;
    actor: CanvasOperationActor;
    requestId: string;
    projectId: string;
    baseRevision: number;
    previousRevision: number;
    revision: number;
    processedAt: string;
    operationResults: CanvasOperationResultItem[];
    error?: CanvasOperationError;
};

export type CanvasProtocolUndoSnapshot = {
    title?: string;
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    locks: Record<string, CanvasNodeLock>;
    tasks: Record<string, CanvasProtocolTask>;
};

export type CanvasOperationAuditEntry = {
    batch: CanvasOperationBatch;
    result: CanvasOperationBatchResult;
    undoSnapshot?: CanvasProtocolUndoSnapshot;
    undoneByRequestId?: string;
};

export type CanvasProcessedRequest = {
    fingerprint: string;
    result: CanvasOperationBatchResult;
};

export type CanvasOperationState = {
    version: typeof CANVAS_OPERATION_PROTOCOL_VERSION;
    revision: number;
    locks: Record<string, CanvasNodeLock>;
    tasks: Record<string, CanvasProtocolTask>;
    requests: Record<string, CanvasProcessedRequest>;
    audit: CanvasOperationAuditEntry[];
};

export type CanvasProtocolProject = {
    id: string;
    title?: string;
    updatedAt: string;
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
    operationState?: CanvasOperationState;
};

export type CanvasOperationOutcome<TProject extends CanvasProtocolProject> = {
    project: TProject & { operationState: CanvasOperationState };
    result: CanvasOperationBatchResult;
};

type ApplyOptions = {
    now?: () => string;
};

class OperationFailure extends Error {
    constructor(readonly error: CanvasOperationError) {
        super(error.message);
    }
}

export function createCanvasOperationState(project: Pick<CanvasProtocolProject, "nodes">): CanvasOperationState {
    return {
        version: CANVAS_OPERATION_PROTOCOL_VERSION,
        revision: 0,
        locks: {},
        tasks: migrateEmbeddedTasks(project.nodes),
        requests: {},
        audit: [],
    };
}

export function migrateCanvasProject<TProject extends CanvasProtocolProject>(project: TProject): TProject & { operationState: CanvasOperationState } {
    const source = isRecord(project.operationState) ? project.operationState : undefined;
    const fallback = createCanvasOperationState(project);
    const nodeIds = new Set(project.nodes.map((node) => node.id));
    const locks = isRecord(source?.locks) ? (Object.fromEntries(Object.entries(source.locks).filter(([nodeId, lock]) => nodeIds.has(nodeId) && isRecord(lock) && lock.nodeId === nodeId)) as Record<string, CanvasNodeLock>) : fallback.locks;
    const tasks = isRecord(source?.tasks) ? (Object.fromEntries(Object.entries(source.tasks).filter(([, task]) => isRecord(task) && typeof task.id === "string")) as Record<string, CanvasProtocolTask>) : fallback.tasks;
    const requests = isRecord(source?.requests) ? (source.requests as Record<string, CanvasProcessedRequest>) : fallback.requests;
    const audit = Array.isArray(source?.audit) ? (source.audit as CanvasOperationAuditEntry[]) : fallback.audit;

    return {
        ...project,
        nodes: project.nodes.map(canonicalizeStoredNode),
        operationState: {
            version: CANVAS_OPERATION_PROTOCOL_VERSION,
            revision: nonNegativeInteger(source?.revision) ?? 0,
            locks,
            tasks: { ...fallback.tasks, ...tasks },
            requests,
            audit,
        },
    };
}

export function rebindCanvasProjectIdentity<TProject extends CanvasProtocolProject>(sourceProject: TProject, projectId: string): TProject & { operationState: CanvasOperationState } {
    const project = clone(migrateCanvasProject(sourceProject));
    project.id = projectId;
    project.operationState.audit = project.operationState.audit.map((sourceEntry) => {
        const entry = clone(sourceEntry);
        entry.batch.projectId = projectId;
        entry.result.projectId = projectId;
        return entry;
    });
    project.operationState.requests = Object.fromEntries(
        project.operationState.audit
            .filter((entry) => validId(entry.batch.requestId))
            .map((entry) => [
                entry.batch.requestId,
                {
                    fingerprint: fingerprintBatch(entry.batch),
                    result: clone(entry.result),
                },
            ]),
    );
    return project;
}

export function buildCanvasStructureOperations(current: Pick<CanvasProtocolProject, "nodes" | "connections">, nextNodes: CanvasNodeData[], nextConnections: CanvasConnection[]): CanvasOperation[] {
    const operations: CanvasOperation[] = [];
    const canonicalCurrentNodes = current.nodes.map(canonicalizeStoredNode);
    const canonicalNextNodes = nextNodes.map(canonicalizeStoredNode);
    const currentNodes = new Map(canonicalCurrentNodes.map((node) => [node.id, node]));
    const targetNodes = new Map(canonicalNextNodes.map((node) => [node.id, node]));
    const currentConnections = new Map(current.connections.map((connection) => [connection.id, connection]));
    const targetConnections = new Map(nextConnections.map((connection) => [connection.id, connection]));

    current.connections.forEach((connection) => {
        const target = targetConnections.get(connection.id);
        if (!target || stableStringify(target) !== stableStringify(connection)) {
            operations.push({ type: "connection.delete", connectionId: connection.id });
        }
    });
    canonicalCurrentNodes.forEach((node) => {
        if (!targetNodes.has(node.id)) operations.push({ type: "node.delete", nodeId: node.id });
    });
    canonicalNextNodes.forEach((node) => {
        const previous = currentNodes.get(node.id);
        if (!previous) {
            operations.push({ type: "node.create", node: clone(node) });
            return;
        }
        if (stableStringify(previous) === stableStringify(node)) return;
        operations.push({
            type: "node.update",
            nodeId: node.id,
            patch: {
                type: node.type,
                title: node.title,
                position: clone(node.position),
                width: node.width,
                height: node.height,
                metadata: clone(node.metadata || {}),
            },
        });
    });
    nextConnections.forEach((connection) => {
        const previous = currentConnections.get(connection.id);
        if (!previous || stableStringify(previous) !== stableStringify(connection)) {
            operations.push({ type: "connection.create", connection: clone(connection) });
        }
    });
    return operations;
}

export function applyCanvasOperationBatch<TProject extends CanvasProtocolProject>(sourceProject: TProject, batch: CanvasOperationBatch, options: ApplyOptions = {}): CanvasOperationOutcome<TProject> {
    const project = clone(migrateCanvasProject(sourceProject));
    const state = project.operationState;
    const processedAt = options.now?.() || new Date().toISOString();
    const requestId = typeof batch?.requestId === "string" ? batch.requestId.trim() : "";
    const fingerprint = fingerprintBatch(batch);

    if (requestId && state.requests[requestId]) {
        const previous = state.requests[requestId];
        if (previous.fingerprint === fingerprint) {
            return { project, result: { ...clone(previous.result), duplicate: true } };
        }
        return {
            project,
            result: rejectedResult(batch, state.revision, processedAt, {
                code: "request_id_reused",
                message: `request id ${requestId} 已用于其他批次`,
            }),
        };
    }

    const envelopeError = validateBatchEnvelope(project, batch);
    if (envelopeError) return recordRejection(project, batch, fingerprint, processedAt, envelopeError);
    if (batch.baseRevision !== state.revision) {
        return recordRejection(project, batch, fingerprint, processedAt, {
            code: "stale_revision",
            message: `画布 revision 已从 ${batch.baseRevision} 变为 ${state.revision}`,
            currentRevision: state.revision,
        });
    }

    const previousRevision = state.revision;
    const undoSnapshot = snapshot(project);
    const operationResults: CanvasOperationResultItem[] = [];

    try {
        batch.operations.forEach((operation, operationIndex) => {
            try {
                operationResults.push(applyOperation(project, batch, operation, processedAt));
            } catch (error) {
                if (error instanceof OperationFailure) {
                    throw new OperationFailure({ ...error.error, operationIndex });
                }
                throw error;
            }
        });
    } catch (error) {
        const operationError = error instanceof OperationFailure ? error.error : { code: "invalid_batch" as const, message: error instanceof Error ? error.message : "画布批次执行失败" };
        const cleanProject = clone(migrateCanvasProject(sourceProject));
        return recordRejection(cleanProject, batch, fingerprint, processedAt, operationError);
    }

    state.revision = previousRevision + 1;
    project.updatedAt = processedAt;
    const result: CanvasOperationBatchResult = {
        ok: true,
        status: "applied",
        duplicate: false,
        actor: batch.actor,
        requestId: batch.requestId,
        projectId: batch.projectId,
        baseRevision: batch.baseRevision,
        previousRevision,
        revision: state.revision,
        processedAt,
        operationResults,
    };
    const entry: CanvasOperationAuditEntry = {
        batch: clone(batch),
        result: clone(result),
        ...(batch.actor === "agent" ? { undoSnapshot } : {}),
    };
    state.audit.push(entry);
    state.requests[requestId] = { fingerprint, result: clone(result) };
    return { project, result };
}

function applyOperation<TProject extends CanvasProtocolProject>(project: TProject & { operationState: CanvasOperationState }, batch: CanvasOperationBatch, operation: CanvasOperation, processedAt: string): CanvasOperationResultItem {
    switch (operation.type) {
        case "project.update": {
            if (typeof operation.title !== "string" || !operation.title.trim() || operation.title.length > 256) {
                fail("invalid_batch", "画布标题无效");
            }
            project.title = operation.title.trim();
            return { type: operation.type, title: project.title };
        }
        case "node.create": {
            validateNode(operation.node);
            if (findNode(project, operation.node.id)) fail("node_exists", `节点 ${operation.node.id} 已存在`, { nodeId: operation.node.id });
            project.nodes.push(clone(operation.node));
            return { type: operation.type, nodeId: operation.node.id };
        }
        case "node.update": {
            const index = project.nodes.findIndex((node) => node.id === operation.nodeId);
            if (index < 0) fail("node_not_found", `找不到节点 ${operation.nodeId}`, { nodeId: operation.nodeId });
            assertAgentMayTouchNode(project, batch.actor, operation.nodeId);
            const patch = clone(operation.patch);
            const nextNode = {
                ...project.nodes[index],
                ...patch,
                ...(patch.metadata ? { metadata: { ...project.nodes[index].metadata, ...patch.metadata } } : {}),
                id: operation.nodeId,
            };
            validateNode(nextNode);
            project.nodes[index] = nextNode;
            return { type: operation.type, nodeId: operation.nodeId };
        }
        case "node.delete": {
            const node = findNode(project, operation.nodeId);
            if (!node) fail("node_not_found", `找不到节点 ${operation.nodeId}`, { nodeId: operation.nodeId });
            assertAgentMayTouchNode(project, batch.actor, operation.nodeId);
            const affectedConnections = project.connections.filter((connection) => connection.fromNodeId === operation.nodeId || connection.toNodeId === operation.nodeId);
            affectedConnections.forEach((connection) => {
                assertAgentMayTouchNode(project, batch.actor, connection.fromNodeId);
                assertAgentMayTouchNode(project, batch.actor, connection.toNodeId);
            });
            if (node.type === "group") {
                project.nodes.filter((item) => item.metadata?.groupId === operation.nodeId).forEach((item) => assertAgentMayTouchNode(project, batch.actor, item.id));
            }
            const affectedConnectionIds = affectedConnections.map((connection) => connection.id);
            project.nodes = project.nodes.filter((item) => item.id !== operation.nodeId).map((item) => (item.metadata?.groupId === operation.nodeId ? { ...item, metadata: { ...item.metadata, groupId: undefined } } : item));
            project.connections = project.connections.filter((connection) => connection.fromNodeId !== operation.nodeId && connection.toNodeId !== operation.nodeId);
            delete project.operationState.locks[operation.nodeId];
            Object.entries(project.operationState.tasks).forEach(([taskId, task]) => {
                if (task.nodeId === operation.nodeId) delete project.operationState.tasks[taskId];
            });
            return { type: operation.type, nodeId: operation.nodeId, nodeIds: [operation.nodeId], connectionIds: affectedConnectionIds };
        }
        case "connection.create": {
            const connection = operation.connection;
            if (!validId(connection?.id) || !validId(connection?.fromNodeId) || !validId(connection?.toNodeId)) {
                fail("invalid_connection", "连线必须包含有效 id、fromNodeId 和 toNodeId");
            }
            if (connection.fromNodeId === connection.toNodeId) fail("invalid_connection", "节点不能连接到自身");
            if (!findNode(project, connection.fromNodeId)) fail("node_not_found", `找不到节点 ${connection.fromNodeId}`, { nodeId: connection.fromNodeId });
            if (!findNode(project, connection.toNodeId)) fail("node_not_found", `找不到节点 ${connection.toNodeId}`, { nodeId: connection.toNodeId });
            assertAgentMayTouchNode(project, batch.actor, connection.fromNodeId);
            assertAgentMayTouchNode(project, batch.actor, connection.toNodeId);
            const byId = project.connections.find((item) => item.id === connection.id);
            if (byId) {
                if (byId.fromNodeId === connection.fromNodeId && byId.toNodeId === connection.toNodeId) {
                    return { type: operation.type, connectionId: byId.id, alreadyExists: true };
                }
                fail("connection_exists", `连线 id ${connection.id} 已存在`);
            }
            const byEndpoints = project.connections.find((item) => item.fromNodeId === connection.fromNodeId && item.toNodeId === connection.toNodeId);
            if (byEndpoints) return { type: operation.type, connectionId: byEndpoints.id, alreadyExists: true };
            project.connections.push(clone(connection));
            return { type: operation.type, connectionId: connection.id };
        }
        case "connection.delete": {
            const connection = project.connections.find((item) => item.id === operation.connectionId);
            if (!connection) fail("connection_not_found", `找不到连线 ${operation.connectionId}`);
            assertAgentMayTouchNode(project, batch.actor, connection.fromNodeId);
            assertAgentMayTouchNode(project, batch.actor, connection.toNodeId);
            project.connections = project.connections.filter((item) => item.id !== operation.connectionId);
            return { type: operation.type, connectionId: operation.connectionId };
        }
        case "layout.apply": {
            if (!isRecord(operation.positions) || !Object.keys(operation.positions).length) {
                fail("invalid_batch", "layout.apply 至少需要一个节点位置");
            }
            const positionEntries = Object.entries(operation.positions);
            positionEntries.forEach(([nodeId, position]) => {
                if (!findNode(project, nodeId)) fail("node_not_found", `找不到节点 ${nodeId}`, { nodeId });
                assertAgentMayTouchNode(project, batch.actor, nodeId);
                if (!validPosition(position)) fail("invalid_node", `节点 ${nodeId} 位置无效`, { nodeId });
            });
            project.nodes = project.nodes.map((node) => (operation.positions[node.id] ? { ...node, position: clone(operation.positions[node.id]) } : node));
            return { type: operation.type, nodeIds: positionEntries.map(([nodeId]) => nodeId) };
        }
        case "task.start": {
            const task = operation.task;
            if (!validId(task?.id) || !validId(task?.nodeId) || !validId(task?.kind)) fail("invalid_batch", "task.start 缺少有效任务字段");
            if (!findNode(project, task.nodeId)) fail("node_not_found", `找不到节点 ${task.nodeId}`, { nodeId: task.nodeId });
            assertAgentMayTouchNode(project, batch.actor, task.nodeId);
            if (project.operationState.tasks[task.id]) fail("task_exists", `任务 ${task.id} 已存在`);
            project.operationState.tasks[task.id] = {
                ...clone(task),
                status: task.status || "queued",
                createdAt: processedAt,
                updatedAt: processedAt,
                requestId: task.requestId || batch.requestId,
            };
            return { type: operation.type, nodeId: task.nodeId, taskId: task.id };
        }
        case "task.cancel": {
            const task = project.operationState.tasks[operation.taskId];
            if (!task) fail("task_not_found", `找不到任务 ${operation.taskId}`);
            assertAgentMayTouchNode(project, batch.actor, task.nodeId);
            if (["cancelled", "succeeded", "failed"].includes(task.status)) fail("task_terminal", `任务 ${operation.taskId} 已进入终态`);
            project.operationState.tasks[operation.taskId] = {
                ...task,
                status: "cancel_requested",
                updatedAt: processedAt,
                details: operation.reason ? { ...task.details, cancelReason: operation.reason } : task.details,
            };
            return { type: operation.type, nodeId: task.nodeId, taskId: operation.taskId };
        }
        case "task.update": {
            if (batch.actor !== "system") fail("task_update_forbidden", "只有 system 批次可以回填任务运行状态");
            const task = project.operationState.tasks[operation.taskId];
            if (!task) fail("task_not_found", `找不到任务 ${operation.taskId}`);
            if (!["running", "cancelled", "succeeded", "failed"].includes(operation.status)) {
                fail("invalid_batch", `任务状态 ${operation.status} 不可由执行器回填`);
            }
            project.operationState.tasks[operation.taskId] = {
                ...task,
                status: operation.status,
                updatedAt: processedAt,
                details: operation.details ? { ...task.details, ...clone(operation.details) } : task.details,
            };
            return { type: operation.type, nodeId: task.nodeId, taskId: operation.taskId };
        }
        case "lock.set": {
            if (batch.actor !== "human") fail("lock_forbidden", "只有人工批次可以锁定或解锁节点", { nodeId: operation.nodeId });
            if (!findNode(project, operation.nodeId)) fail("node_not_found", `找不到节点 ${operation.nodeId}`, { nodeId: operation.nodeId });
            if (operation.locked) {
                project.operationState.locks[operation.nodeId] = {
                    nodeId: operation.nodeId,
                    lockedAt: processedAt,
                    requestId: batch.requestId,
                    actor: "human",
                };
            } else {
                delete project.operationState.locks[operation.nodeId];
            }
            return { type: operation.type, nodeId: operation.nodeId, locked: operation.locked };
        }
        case "batch.undo": {
            if (batch.operations.length !== 1) fail("undo_forbidden", "batch.undo 必须是批次内唯一操作");
            if (batch.actor === "agent") fail("undo_forbidden", "Agent 不能自行撤销已提交批次");
            const target = project.operationState.audit.find((entry) => entry.batch.requestId === operation.targetRequestId);
            if (!target || !target.result.ok || !target.undoSnapshot) fail("undo_not_found", `找不到可撤销的 Agent 批次 ${operation.targetRequestId}`);
            if (target.batch.actor !== "agent") fail("undo_forbidden", "只能撤销 Agent 批次");
            if (target.undoneByRequestId) fail("already_undone", `Agent 批次 ${operation.targetRequestId} 已撤销`);
            if (target.result.revision !== batch.baseRevision) {
                fail("undo_forbidden", "Agent 批次之后已有新的画布修改，为避免覆盖人工结果不能直接恢复快照");
            }
            project.nodes = clone(target.undoSnapshot.nodes);
            project.connections = clone(target.undoSnapshot.connections);
            if (target.undoSnapshot.title !== undefined) project.title = target.undoSnapshot.title;
            project.operationState.locks = clone(target.undoSnapshot.locks);
            project.operationState.tasks = clone(target.undoSnapshot.tasks);
            target.undoneByRequestId = batch.requestId;
            return { type: operation.type, targetRequestId: operation.targetRequestId };
        }
        default:
            fail("invalid_batch", "不支持的画布操作");
    }
}

function validateBatchEnvelope(project: CanvasProtocolProject, batch: CanvasOperationBatch): CanvasOperationError | undefined {
    if (!isRecord(batch) || batch.protocolVersion !== CANVAS_OPERATION_PROTOCOL_VERSION) {
        return { code: "invalid_batch", message: "画布批次协议版本无效" };
    }
    if (!validId(batch.requestId) || !validId(batch.projectId) || !["human", "agent", "system"].includes(batch.actor)) {
        return { code: "invalid_batch", message: "画布批次缺少 actor、request id 或 project id" };
    }
    if (batch.projectId !== project.id) {
        return { code: "project_mismatch", message: `批次属于项目 ${batch.projectId}，当前项目为 ${project.id}` };
    }
    if (nonNegativeInteger(batch.baseRevision) === undefined) {
        return { code: "invalid_batch", message: "base revision 必须是非负整数" };
    }
    if (typeof batch.timestamp !== "string" || !Number.isFinite(Date.parse(batch.timestamp))) {
        return { code: "invalid_batch", message: "批次时间必须是有效 ISO 时间" };
    }
    if (!Array.isArray(batch.operations) || !batch.operations.length) {
        return { code: "invalid_batch", message: "画布批次至少需要一个操作" };
    }
    return undefined;
}

function recordRejection<TProject extends CanvasProtocolProject>(
    project: TProject & { operationState: CanvasOperationState },
    batch: CanvasOperationBatch,
    fingerprint: string,
    processedAt: string,
    error: CanvasOperationError,
): CanvasOperationOutcome<TProject> {
    const result = rejectedResult(batch, project.operationState.revision, processedAt, error);
    project.updatedAt = processedAt;
    project.operationState.audit.push({ batch: clone(batch), result: clone(result) });
    if (validId(batch.requestId)) {
        project.operationState.requests[batch.requestId.trim()] = { fingerprint, result: clone(result) };
    }
    return { project, result };
}

function rejectedResult(batch: CanvasOperationBatch, revision: number, processedAt: string, error: CanvasOperationError): CanvasOperationBatchResult {
    return {
        ok: false,
        status: "rejected",
        duplicate: false,
        actor: batch?.actor || "system",
        requestId: typeof batch?.requestId === "string" ? batch.requestId : "",
        projectId: typeof batch?.projectId === "string" ? batch.projectId : "",
        baseRevision: nonNegativeInteger(batch?.baseRevision) ?? -1,
        previousRevision: revision,
        revision,
        processedAt,
        operationResults: [],
        error,
    };
}

function snapshot(project: CanvasProtocolProject & { operationState: CanvasOperationState }): CanvasProtocolUndoSnapshot {
    return {
        title: project.title,
        nodes: clone(project.nodes),
        connections: clone(project.connections),
        locks: clone(project.operationState.locks),
        tasks: clone(project.operationState.tasks),
    };
}

function assertAgentMayTouchNode(project: CanvasProtocolProject & { operationState: CanvasOperationState }, actor: CanvasOperationActor, nodeId: string) {
    if (actor === "agent" && project.operationState.locks[nodeId]) {
        fail("locked_node", `节点 ${nodeId} 已被人工锁定，Agent 不得修改`, { nodeId });
    }
}

function validateNode(node: CanvasNodeData) {
    if (!isRecord(node) || !validId(node.id) || !validId(node.type) || typeof node.title !== "string" || !validPosition(node.position) || !Number.isFinite(node.width) || node.width <= 0 || !Number.isFinite(node.height) || node.height <= 0) {
        fail("invalid_node", "节点必须包含有效 id、type、position、width 和 height", { nodeId: node?.id });
    }
}

function findNode(project: CanvasProtocolProject, nodeId: string) {
    return project.nodes.find((node) => node.id === nodeId);
}

function validPosition(value: unknown): value is Position {
    return isRecord(value) && Number.isFinite(value.x) && Number.isFinite(value.y);
}

function validId(value: unknown): value is string {
    return typeof value === "string" && value.trim().length > 0 && value.length <= 256;
}

function nonNegativeInteger(value: unknown) {
    return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : undefined;
}

function fail(code: CanvasOperationError["code"], message: string, extra: Partial<CanvasOperationError> = {}): never {
    throw new OperationFailure({ code, message, ...extra });
}

function fingerprintBatch(batch: CanvasOperationBatch) {
    return stableStringify({
        protocolVersion: batch?.protocolVersion,
        actor: batch?.actor,
        projectId: batch?.projectId,
        operations: batch?.operations,
    });
}

function stableStringify(value: unknown): string {
    if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
    if (isRecord(value)) {
        return `{${Object.keys(value)
            .sort()
            .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
            .join(",")}}`;
    }
    return JSON.stringify(value) ?? "null";
}

function migrateEmbeddedTasks(nodes: CanvasNodeData[]) {
    const tasks: Record<string, CanvasProtocolTask> = {};
    nodes.forEach((node) => {
        const metadata = node.metadata;
        const embedded = embeddedTask(metadata);
        if (!embedded || tasks[embedded.id]) return;
        const timestamp = typeof metadata?.startedAt === "number" && Number.isFinite(metadata.startedAt) ? new Date(metadata.startedAt).toISOString() : new Date(0).toISOString();
        tasks[embedded.id] = {
            id: embedded.id,
            nodeId: node.id,
            kind: embedded.kind,
            status: embeddedStatus(metadata?.status),
            createdAt: timestamp,
            updatedAt: timestamp,
            details: { migratedFromNodeMetadata: true },
        };
    });
    return tasks;
}

function embeddedTask(metadata?: CanvasNodeMetadata) {
    if (metadata?.localTaskId) return { id: metadata.localTaskId, kind: metadata.localTaskKind || "local" };
    if (metadata?.videoTaskId) return { id: metadata.videoTaskId, kind: "video" };
    if (metadata?.imageTaskId) return { id: metadata.imageTaskId, kind: "image" };
    if (metadata?.audioTaskId) return { id: metadata.audioTaskId, kind: "audio" };
    return undefined;
}

function embeddedStatus(status?: CanvasNodeMetadata["status"]): CanvasProtocolTaskStatus {
    if (status === "loading") return "running";
    if (status === "success") return "succeeded";
    if (status === "error") return "failed";
    return "queued";
}

function canonicalizeStoredNode(node: CanvasNodeData): CanvasNodeData {
    const storageKey = node.metadata?.storageKey;
    const content = node.metadata?.content;
    if (
        typeof storageKey !== "string"
        || !storageKey
        || storageKey.startsWith("server:")
        || typeof content !== "string"
        || (!content.startsWith("blob:") && !content.startsWith("data:"))
    ) {
        return node;
    }
    return {
        ...node,
        metadata: {
            ...node.metadata,
            content: storageKey,
        },
    };
}

function isRecord(value: unknown): value is Record<string, any> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

function clone<T>(value: T): T {
    if (value === undefined) return value;
    if (typeof structuredClone === "function") return structuredClone(value);
    return JSON.parse(JSON.stringify(value)) as T;
}
