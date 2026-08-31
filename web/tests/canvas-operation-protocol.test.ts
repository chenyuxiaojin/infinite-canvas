import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasConnection, type CanvasNodeData } from "../src/app/(user)/canvas/types";
import {
    CANVAS_OPERATION_PROTOCOL_VERSION,
    applyCanvasOperationBatch,
    buildCanvasStructureOperations,
    migrateCanvasProject,
    rebindCanvasProjectIdentity,
    type CanvasOperation,
    type CanvasOperationActor,
    type CanvasOperationBatch,
    type CanvasProtocolProject,
} from "../src/app/(user)/canvas/protocol/canvas-operation-protocol";

const TIME = "2026-08-30T00:00:00.000Z";

function node(id: string, title = id, x = 0): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Text,
        title,
        position: { x, y: 0 },
        width: 240,
        height: 160,
        metadata: { content: title },
    };
}

function project(nodes: CanvasNodeData[] = [], connections: CanvasConnection[] = []) {
    return migrateCanvasProject({
        id: "project-1",
        title: "协议测试",
        createdAt: TIME,
        updatedAt: TIME,
        nodes,
        connections,
    } as CanvasProtocolProject & { title: string; createdAt: string });
}

function batch(actor: CanvasOperationActor, requestId: string, baseRevision: number, operations: CanvasOperation[]): CanvasOperationBatch {
    return {
        protocolVersion: CANVAS_OPERATION_PROTOCOL_VERSION,
        actor,
        requestId,
        projectId: "project-1",
        baseRevision,
        timestamp: TIME,
        operations,
    };
}

describe("人与 Agent 共用画布操作协议", () => {
    test("UI 差异和 Agent 批次使用同一 reducer", () => {
        const initial = project();
        const humanNode = node("human-node", "人工节点");
        const humanOperations = buildCanvasStructureOperations(initial, [humanNode], []);
        const human = applyCanvasOperationBatch(initial, batch("human", "human-create", 0, humanOperations), { now: () => TIME });
        const agentNode = node("agent-node", "Agent 节点", 320);
        const agent = applyCanvasOperationBatch(
            human.project,
            batch("agent", "agent-create", 1, [
                { type: "node.create", node: agentNode },
                { type: "connection.create", connection: { id: "connection-1", fromNodeId: humanNode.id, toNodeId: agentNode.id } },
            ]),
            { now: () => TIME },
        );

        expect(human.result.ok).toBe(true);
        expect(agent.result.ok).toBe(true);
        expect(agent.project.nodes.map((item) => item.id)).toEqual(["human-node", "agent-node"]);
        expect(agent.project.connections).toHaveLength(1);
        expect(agent.project.operationState.revision).toBe(2);
        expect(agent.project.operationState.audit.map((entry) => entry.batch.actor)).toEqual(["human", "agent"]);
    });

    test("重复 request id 返回原回执且不重复执行", () => {
        const request = batch("agent", "same-request", 0, [{ type: "node.create", node: node("n1") }]);
        const first = applyCanvasOperationBatch(project(), request, { now: () => TIME });
        const retry = applyCanvasOperationBatch(first.project, { ...request, baseRevision: 1, timestamp: "2026-08-30T00:01:00.000Z" }, { now: () => TIME });

        expect(first.result.ok).toBe(true);
        expect(retry.result.ok).toBe(true);
        expect(retry.result.duplicate).toBe(true);
        expect(retry.project.nodes).toHaveLength(1);
        expect(retry.project.operationState.revision).toBe(1);
        expect(retry.project.operationState.audit).toHaveLength(1);
    });

    test("人工锁定会拒绝 Agent 修改，人工修改仍优先", () => {
        const initial = project([node("locked", "原标题")]);
        const locked = applyCanvasOperationBatch(initial, batch("human", "lock", 0, [{ type: "lock.set", nodeId: "locked", locked: true }]), { now: () => TIME });
        const rejected = applyCanvasOperationBatch(locked.project, batch("agent", "agent-update", 1, [{ type: "node.update", nodeId: "locked", patch: { title: "Agent 标题" } }]), { now: () => TIME });
        const human = applyCanvasOperationBatch(rejected.project, batch("human", "human-update", 1, [{ type: "node.update", nodeId: "locked", patch: { title: "人工标题" } }]), { now: () => TIME });

        expect(rejected.result.error?.code).toBe("locked_node");
        expect(rejected.project.nodes[0].title).toBe("原标题");
        expect(rejected.project.operationState.revision).toBe(1);
        expect(human.result.ok).toBe(true);
        expect(human.project.nodes[0].title).toBe("人工标题");
        expect(human.project.operationState.revision).toBe(2);
    });

    test("过期 revision 被拒绝，失败回执在重启后仍可幂等读取", () => {
        const first = applyCanvasOperationBatch(project(), batch("human", "first", 0, [{ type: "node.create", node: node("n1") }]), { now: () => TIME });
        const staleBatch = batch("agent", "stale", 0, [{ type: "node.create", node: node("n2") }]);
        const stale = applyCanvasOperationBatch(first.project, staleBatch, { now: () => TIME });
        const reloaded = migrateCanvasProject(JSON.parse(JSON.stringify(stale.project)));
        const retry = applyCanvasOperationBatch(reloaded, staleBatch, { now: () => TIME });

        expect(stale.result.error?.code).toBe("stale_revision");
        expect(stale.result.error?.currentRevision).toBe(1);
        expect(stale.project.nodes.map((item) => item.id)).toEqual(["n1"]);
        expect(retry.result.error?.code).toBe("stale_revision");
        expect(retry.result.duplicate).toBe(true);
        expect(retry.project.operationState.audit).toHaveLength(2);
    });

    test("人工可精确撤销 Agent 批次并保留审计链", () => {
        const initial = project([node("n1", "撤销前")]);
        const agent = applyCanvasOperationBatch(initial, batch("agent", "agent-change", 0, [{ type: "node.update", nodeId: "n1", patch: { title: "Agent 改动", position: { x: 640, y: 320 } } }]), { now: () => TIME });
        const undone = applyCanvasOperationBatch(agent.project, batch("human", "undo-agent-change", 1, [{ type: "batch.undo", targetRequestId: "agent-change" }]), { now: () => TIME });

        expect(agent.project.operationState.audit[0].undoSnapshot?.nodes[0].title).toBe("撤销前");
        expect(undone.result.ok).toBe(true);
        expect(undone.project.nodes[0].title).toBe("撤销前");
        expect(undone.project.nodes[0].position).toEqual({ x: 0, y: 0 });
        expect(undone.project.operationState.audit[0].undoneByRequestId).toBe("undo-agent-change");
        expect(undone.project.operationState.audit).toHaveLength(2);
        expect(undone.project.operationState.revision).toBe(2);
    });

    test("撤销不会覆盖 Agent 之后的人工修改", () => {
        const initial = project([node("n1", "原始")]);
        const agent = applyCanvasOperationBatch(initial, batch("agent", "agent-first", 0, [{ type: "node.update", nodeId: "n1", patch: { title: "Agent" } }]), { now: () => TIME });
        const human = applyCanvasOperationBatch(agent.project, batch("human", "human-after", 1, [{ type: "node.update", nodeId: "n1", patch: { title: "人工最终版" } }]), { now: () => TIME });
        const undo = applyCanvasOperationBatch(human.project, batch("human", "unsafe-undo", 2, [{ type: "batch.undo", targetRequestId: "agent-first" }]), { now: () => TIME });

        expect(undo.result.error?.code).toBe("undo_forbidden");
        expect(undo.project.nodes[0].title).toBe("人工最终版");
        expect(undo.project.operationState.revision).toBe(2);
    });

    test("媒体任务的 system 回填不会阻止人工撤销对应 Agent 批次", () => {
        const videoNode: CanvasNodeData = {
            id: "agent-video",
            type: CanvasNodeType.Video,
            title: "Agent 视频",
            position: { x: 0, y: 0 },
            width: 320,
            height: 180,
            metadata: { status: "loading", localTaskKind: "agent_video_ingest" },
        };
        const agent = applyCanvasOperationBatch(project(), batch("agent", "agent-video-ingest", 0, [
            { type: "node.create", node: videoNode },
            { type: "task.start", task: { id: "canvas-media-task", nodeId: videoNode.id, kind: "agent_video_ingest" } },
        ]), { now: () => TIME });
        const running = applyCanvasOperationBatch(agent.project, batch("system", "system-media-running", 1, [
            { type: "task.update", taskId: "canvas-media-task", status: "running", details: { runtimeTaskId: "runtime-task" } },
            { type: "node.update", nodeId: videoNode.id, patch: { metadata: { localTaskId: "runtime-task", progress: 15 } } },
        ]), { now: () => TIME });
        const succeeded = applyCanvasOperationBatch(running.project, batch("system", "system-media-succeeded", 2, [
            { type: "task.update", taskId: "canvas-media-task", status: "succeeded" },
            { type: "node.update", nodeId: videoNode.id, patch: { metadata: { content: "local-task:runtime-task", status: "success", progress: 100 } } },
        ]), { now: () => TIME });
        const undone = applyCanvasOperationBatch(succeeded.project, batch("human", "undo-agent-video", 3, [
            { type: "batch.undo", targetRequestId: "agent-video-ingest" },
        ]), { now: () => TIME });

        expect(undone.result.ok).toBe(true);
        expect(undone.project.nodes).toEqual([]);
        expect(undone.project.operationState.tasks).toEqual({});
        expect(undone.project.operationState.audit[0].undoneByRequestId).toBe("undo-agent-video");
        expect(undone.project.operationState.revision).toBe(4);
    });

    test("冲突批次原子拒绝，不留部分布局改动", () => {
        const initial = project([node("free"), node("locked", "locked", 300)]);
        const locked = applyCanvasOperationBatch(initial, batch("human", "lock-layout", 0, [{ type: "lock.set", nodeId: "locked", locked: true }]), { now: () => TIME });
        const layout = applyCanvasOperationBatch(
            locked.project,
            batch("agent", "agent-layout", 1, [
                {
                    type: "layout.apply",
                    positions: { free: { x: 1000, y: 1000 }, locked: { x: 1200, y: 1000 } },
                },
            ]),
            { now: () => TIME },
        );

        expect(layout.result.error?.code).toBe("locked_node");
        expect(layout.result.error?.operationIndex).toBe(0);
        expect(layout.project.nodes.find((item) => item.id === "free")?.position).toEqual({ x: 0, y: 0 });
        expect(layout.project.nodes.find((item) => item.id === "locked")?.position).toEqual({ x: 300, y: 0 });
    });

    test("任务发起和取消是幂等、可审计的画布操作", () => {
        const initial = project([node("media")]);
        const started = applyCanvasOperationBatch(initial, batch("agent", "task-start", 0, [{ type: "task.start", task: { id: "task-1", nodeId: "media", kind: "video" } }]), { now: () => TIME });
        const cancelled = applyCanvasOperationBatch(started.project, batch("human", "task-cancel", 1, [{ type: "task.cancel", taskId: "task-1", reason: "人工取消" }]), { now: () => TIME });
        const completedCancellation = applyCanvasOperationBatch(cancelled.project, batch("system", "task-cancelled", 2, [{ type: "task.update", taskId: "task-1", status: "cancelled", details: { executorReceipt: "cancelled" } }]), { now: () => TIME });

        expect(started.project.operationState.tasks["task-1"].status).toBe("queued");
        expect(cancelled.project.operationState.tasks["task-1"].status).toBe("cancel_requested");
        expect(cancelled.project.operationState.tasks["task-1"].details?.cancelReason).toBe("人工取消");
        expect(completedCancellation.project.operationState.tasks["task-1"].status).toBe("cancelled");
        expect(completedCancellation.project.operationState.tasks["task-1"].details?.executorReceipt).toBe("cancelled");
        expect(completedCancellation.project.operationState.revision).toBe(3);
    });

    test("Bridge 映射可经公共协议更新标题并合并节点 metadata", () => {
        const initial = project([{ ...node("n1"), metadata: { content: "旧内容", fontSize: 18 } }]);
        const changed = applyCanvasOperationBatch(initial, batch("agent", "bridge-edit", 0, [
            { type: "project.update", title: "Agent 工程名" },
            { type: "node.update", nodeId: "n1", patch: { metadata: { content: "新内容", prompt: "新内容" } } },
        ]), { now: () => TIME });
        const undone = applyCanvasOperationBatch(changed.project, batch("human", "undo-bridge-edit", 1, [{ type: "batch.undo", targetRequestId: "bridge-edit" }]), { now: () => TIME });

        expect(changed.project.title).toBe("Agent 工程名");
        expect(changed.project.nodes[0].metadata).toMatchObject({ content: "新内容", prompt: "新内容", fontSize: 18 });
        expect(undone.project.title).toBe("协议测试");
        expect(undone.project.nodes[0].metadata).toMatchObject({ content: "旧内容", fontSize: 18 });
    });

    test("本地媒体恢复生成的临时 URL 不会制造画布 revision", () => {
        const stored = project([{
            ...node("media"),
            type: CanvasNodeType.Video,
            metadata: { content: "blob:http://127.0.0.1:3210/old", storageKey: "local-task:media-1" },
        }]);
        const hydrated = [{
            ...stored.nodes[0],
            metadata: { ...stored.nodes[0].metadata, content: "blob:http://127.0.0.1:3210/new", errorDetails: undefined },
        }];

        expect(stored.nodes[0].metadata?.content).toBe("local-task:media-1");
        expect(buildCanvasStructureOperations(stored, hydrated, [])).toEqual([]);
        expect(stored.operationState.revision).toBe(0);
    });

    test("本机引用只持久化受控 asset，Range 播放 URL 不会制造 revision", () => {
        const localMedia = {
            assetId: "asset-0123456789abcdef0123456789abcdef",
            storageKey: "local-ref:asset-0123456789abcdef0123456789abcdef",
            rootId: "root-fixture",
            relativePath: "shots/clip.mp4",
            sha256: "a".repeat(64),
            mimeType: "video/mp4",
            bytes: 1024,
            fileName: "clip.mp4",
            width: 1920,
            height: 1080,
            durationMs: 1000,
            mode: "reference" as const,
        };
        const stored = project([{
            ...node("local-media"),
            type: CanvasNodeType.Video,
            metadata: { content: localMedia.storageKey, storageKey: localMedia.storageKey, localMedia },
        }]);
        const hydrated = [{
            ...stored.nodes[0],
            metadata: {
                ...stored.nodes[0].metadata,
                content: "http://127.0.0.1:3213/v1/media/asset?token=ephemeral",
                localMediaRuntime: { status: "available" as const, playbackUrl: "http://127.0.0.1:3213/v1/media/asset?token=ephemeral" },
            },
        }];

        expect(buildCanvasStructureOperations(stored, hydrated, [])).toEqual([]);
        expect(stored.nodes[0].metadata?.content).toBe(localMedia.storageKey);
        expect(stored.nodes[0].metadata?.localMediaRuntime).toBeUndefined();
        expect(stored.operationState.revision).toBe(0);
    });

    test("Agent 操作拒绝绝对路径和目录穿越引用", () => {
        const invalid = {
            ...node("invalid-media"),
            type: CanvasNodeType.Video,
            metadata: {
                storageKey: "local-ref:asset-0123456789abcdef0123456789abcdef",
                localMedia: {
                    assetId: "asset-0123456789abcdef0123456789abcdef",
                    storageKey: "local-ref:asset-0123456789abcdef0123456789abcdef",
                    rootId: "root-fixture",
                    relativePath: "../private/clip.mp4",
                    sha256: "a".repeat(64),
                    mimeType: "video/mp4",
                    bytes: 1024,
                    fileName: "clip.mp4",
                    mode: "reference" as const,
                },
            },
        };
        const result = applyCanvasOperationBatch(project(), batch("agent", "invalid-local-path", 0, [{ type: "node.create", node: invalid }]), { now: () => TIME });

        expect(result.result.error?.code).toBe("invalid_node");
        expect(result.project.nodes).toEqual([]);
        expect(result.project.operationState.revision).toBe(0);
    });

    test("Bridge 画布任务已关联 runtime task 时不迁移出第二份任务", () => {
        const source = project([{
            ...node("media"),
            type: CanvasNodeType.Video,
            metadata: {
                status: "success",
                localTaskId: "runtime-task",
                localTaskKind: "agent_video_ingest",
                localCanvasTaskId: "canvas-task",
            },
        }]);
        source.operationState.tasks = {
            "canvas-task": {
                id: "canvas-task",
                nodeId: "media",
                kind: "agent_video_ingest",
                status: "succeeded",
                createdAt: TIME,
                updatedAt: TIME,
                details: { runtimeTaskId: "runtime-task" },
            },
        };

        const migrated = migrateCanvasProject(JSON.parse(JSON.stringify(source)));
        expect(Object.keys(migrated.operationState.tasks)).toEqual(["canvas-task"]);
        expect(migrated.operationState.tasks["canvas-task"].details?.runtimeTaskId).toBe("runtime-task");
    });

    test("导入副本重绑定工程身份并保留 request id 幂等", () => {
        const created = applyCanvasOperationBatch(project(), batch("agent", "imported-request", 0, [
            { type: "node.create", node: node("imported-node") },
        ]), { now: () => TIME });
        const imported = rebindCanvasProjectIdentity(created.project, "project-copy");
        const retry = applyCanvasOperationBatch(imported, {
            ...batch("agent", "imported-request", 0, [{ type: "node.create", node: node("imported-node") }]),
            projectId: "project-copy",
        }, { now: () => TIME });

        expect(imported.id).toBe("project-copy");
        expect(imported.operationState.audit[0].batch.projectId).toBe("project-copy");
        expect(imported.operationState.audit[0].result.projectId).toBe("project-copy");
        expect(retry.result.ok).toBe(true);
        expect(retry.result.duplicate).toBe(true);
        expect(retry.project.nodes).toHaveLength(1);
        expect(retry.project.operationState.revision).toBe(1);
    });

    test("旧工程迁移不改节点连线，保存后重载一致", () => {
        const oldProject = {
            id: "project-1",
            title: "旧工程",
            createdAt: TIME,
            updatedAt: TIME,
            nodes: [{ ...node("legacy"), metadata: { status: "loading" as const, videoTaskId: "legacy-task" } }],
            connections: [],
        };
        const migrated = migrateCanvasProject(oldProject);
        const edited = applyCanvasOperationBatch(migrated, batch("human", "legacy-edit", 0, [{ type: "node.update", nodeId: "legacy", patch: { title: "旧工程可编辑" } }]), { now: () => TIME });
        const reloaded = migrateCanvasProject(JSON.parse(JSON.stringify(edited.project)));

        expect(migrated.nodes).toEqual(oldProject.nodes);
        expect(migrated.connections).toEqual(oldProject.connections);
        expect(migrated.operationState.tasks["legacy-task"].status).toBe("running");
        expect(reloaded.nodes).toEqual(edited.project.nodes);
        expect(reloaded.connections).toEqual(edited.project.connections);
        expect(reloaded.operationState).toEqual(edited.project.operationState);
        expect(reloaded.nodes[0].title).toBe("旧工程可编辑");
    });
});
