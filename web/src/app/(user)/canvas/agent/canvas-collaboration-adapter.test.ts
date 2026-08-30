import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasNodeData } from "../types";
import {
    CANVAS_OPERATION_PROTOCOL_VERSION,
    applyCanvasOperationBatch,
    migrateCanvasProject,
    type CanvasOperation,
    type CanvasOperationActor,
} from "../protocol/canvas-operation-protocol";
import { canvasCollaborationAdapter } from "./canvas-collaboration-adapter";

const T0 = "2026-08-30T01:00:00.000Z";
const T1 = "2026-08-30T01:00:01.000Z";
const T2 = "2026-08-30T01:00:02.000Z";

function textNode(id = "node-1", content = "旧内容"): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Text,
        title: "剧本",
        position: { x: 10, y: 20 },
        width: 320,
        height: 240,
        metadata: { content, prompt: content },
    };
}

function project() {
    return migrateCanvasProject({ id: "project-1", updatedAt: T0, nodes: [textNode()], connections: [] });
}

function batch(actor: CanvasOperationActor, requestId: string, baseRevision: number, operations: CanvasOperation[]) {
    return {
        protocolVersion: CANVAS_OPERATION_PROTOCOL_VERSION,
        actor,
        requestId,
        projectId: "project-1",
        baseRevision,
        timestamp: T0,
        operations,
    };
}

describe("canvasCollaborationAdapter", () => {
    test("只映射公共 operationState，不暴露第二套 reducer、锁或撤销实现", () => {
        expect("guardMutation" in canvasCollaborationAdapter).toBeFalse();
        expect("markHumanNodes" in canvasCollaborationAdapter).toBeFalse();
        expect("undoLatest" in canvasCollaborationAdapter).toBeFalse();

        const locked = applyCanvasOperationBatch(project(), batch("human", "lock-1", 0, [{ type: "lock.set", nodeId: "node-1", locked: true }]), { now: () => T1 });
        const rejected = applyCanvasOperationBatch(locked.project, batch("agent", "agent-locked", 1, [{ type: "node.update", nodeId: "node-1", patch: { title: "Agent 标题" } }]), { now: () => T2 });
        const view = canvasCollaborationAdapter.toView(rejected.project.operationState, canvasCollaborationAdapter.createStatus(T0), rejected.project.nodes);

        expect(rejected.result.error?.code).toBe("locked_node");
        expect(canvasCollaborationAdapter.nodeCollaboration(rejected.project.operationState, "node-1")).toMatchObject({ lockedByHuman: true, revision: 1, lastEditedBy: "human" });
        expect(view).toMatchObject({ revision: 1, batches: [{ id: "agent-locked", status: "conflict", reversible: false, affectedNodeIds: ["node-1"] }] });
    });

    test("过期 revision 直接映射为冲突，不改写公共 revision", () => {
        const human = applyCanvasOperationBatch(project(), batch("human", "human-edit", 0, [{ type: "node.update", nodeId: "node-1", patch: { title: "人工标题" } }]), { now: () => T1 });
        const stale = applyCanvasOperationBatch(human.project, batch("agent", "agent-stale", 0, [{ type: "node.update", nodeId: "node-1", patch: { title: "Agent 标题" } }]), { now: () => T2 });
        const view = canvasCollaborationAdapter.toView(stale.project.operationState, canvasCollaborationAdapter.createStatus(T0), stale.project.nodes);

        expect(stale.result.error?.code).toBe("stale_revision");
        expect(view.revision).toBe(1);
        expect(view.batches[0]).toMatchObject({ id: "agent-stale", status: "conflict", baseRevision: 0, revision: 1 });
    });

    test("审计历史和撤销状态均来自公共协议", () => {
        const agent = applyCanvasOperationBatch(project(), batch("agent", "agent-edit", 0, [{ type: "node.update", nodeId: "node-1", patch: { title: "Agent 标题" } }]), { now: () => T1 });
        const beforeUndo = canvasCollaborationAdapter.toView(agent.project.operationState, canvasCollaborationAdapter.createStatus(T0), agent.project.nodes);
        const target = canvasCollaborationAdapter.latestUndoTarget(agent.project.operationState);

        expect(beforeUndo.batches[0]).toMatchObject({ id: "agent-edit", status: "success", reversible: true, revision: 1 });
        expect(target?.batch.requestId).toBe("agent-edit");

        const undone = applyCanvasOperationBatch(agent.project, batch("human", "undo-agent", 1, [{ type: "batch.undo", targetRequestId: "agent-edit" }]), { now: () => T2 });
        const afterUndo = canvasCollaborationAdapter.toView(undone.project.operationState, canvasCollaborationAdapter.createStatus(T0), undone.project.nodes);
        expect(afterUndo.revision).toBe(2);
        expect(afterUndo.batches[0].undoneAt).toBe(T2);
        expect(undone.project.nodes[0].title).toBe("剧本");
    });

    test("运行中状态只负责界面反馈，完成结果从新增审计条目归纳", () => {
        const initial = project();
        const started = canvasCollaborationAdapter.beginBatch({
            status: canvasCollaborationAdapter.createStatus(T0),
            operationState: initial.operationState,
            batchId: "run-1",
            summary: "改写剧本",
            now: T0,
        });
        const agent = applyCanvasOperationBatch(initial, batch("agent", "agent-edit", 0, [{ type: "node.update", nodeId: "node-1", patch: { title: "Agent 标题" } }]), { now: () => T1 });
        const status = canvasCollaborationAdapter.finishBatch({ status: started.status, runtime: started.runtime, operationState: agent.project.operationState, now: T2 });

        expect(started.status.state).toBe("running");
        expect(status).toMatchObject({ state: "success", affectedNodeIds: ["node-1"] });
        expect(agent.project.operationState.revision).toBe(1);
    });
});

