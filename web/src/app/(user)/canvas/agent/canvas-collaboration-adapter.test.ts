import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasNodeData } from "../types";
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

describe("canvasCollaborationAdapter", () => {
    test("人工锁定节点会拒绝 Agent 覆盖", () => {
        const node = { ...textNode(), collaboration: { locked: true, revision: 1, lastEditedBy: "human" as const } };
        const state = { ...canvasCollaborationAdapter.createState(T0), revision: 1 };
        const started = canvasCollaborationAdapter.beginBatch({ state, batchId: "batch-1", summary: "改写剧本", nodes: [node], connections: [], now: T0 });

        expect(canvasCollaborationAdapter.guardMutation(started.state, started.runtime, [node.id])).toEqual({
            ok: false,
            code: "locked_node",
            message: "节点 node-1 已被人工锁定，Agent 未修改",
        });
    });

    test("被锁定节点拒绝的 Agent 批次仍进入冲突历史", () => {
        const node = { ...textNode(), collaboration: { locked: true, revision: 1, lastEditedBy: "human" as const } };
        const state = { ...canvasCollaborationAdapter.createState(T0), revision: 1 };
        const started = canvasCollaborationAdapter.beginBatch({ state, batchId: "batch-locked", summary: "改写锁定节点", nodes: [node], connections: [], now: T0 });
        const guard = canvasCollaborationAdapter.guardMutation(started.state, started.runtime, [node.id]);
        expect(guard.ok).toBeFalse();
        const recorded = canvasCollaborationAdapter.recordAction({
            state: started.state,
            runtime: started.runtime,
            actionName: "update_text_node",
            beforeNodes: [node],
            beforeConnections: [],
            afterNodes: [node],
            afterConnections: [],
            result: guard,
            now: T1,
        });
        const finished = canvasCollaborationAdapter.finishBatch({ state: recorded.state, runtime: recorded.runtime, nodes: [node], connections: [], now: T2 });

        expect(finished.batch).toMatchObject({ id: "batch-locked", status: "conflict", reversible: false, affectedNodeIds: [] });
        expect(finished.state.batches).toHaveLength(1);
        expect(finished.state.status.message).toContain("人工锁定");
    });

    test("Agent 执行期间发生人工修改会产生 revision 冲突", () => {
        const node = textNode();
        const started = canvasCollaborationAdapter.beginBatch({
            state: canvasCollaborationAdapter.createState(T0),
            batchId: "batch-1",
            summary: "改写剧本",
            nodes: [node],
            connections: [],
            now: T0,
        });
        const human = canvasCollaborationAdapter.markHumanNodes(started.state, [{ ...node, title: "人工标题" }], [node.id], T1);
        const guard = canvasCollaborationAdapter.guardMutation(human.state, started.runtime);

        expect(guard.ok).toBeFalse();
        expect(guard).toMatchObject({ code: "revision_conflict" });
        expect(human.state.revision).toBe(1);
        expect(human.nodes[0].collaboration?.lastEditedBy).toBe("human");
    });

    test("文本修改生成字段级逆向补丁并可撤销", () => {
        const before = textNode();
        const started = canvasCollaborationAdapter.beginBatch({
            state: canvasCollaborationAdapter.createState(T0),
            batchId: "batch-1",
            summary: "改写剧本",
            nodes: [before],
            connections: [],
            now: T0,
        });
        const changed = { ...before, title: "Agent 标题", metadata: { ...before.metadata, content: "新内容", prompt: "新内容" } };
        const recorded = canvasCollaborationAdapter.recordAction({
            state: started.state,
            runtime: started.runtime,
            actionName: "update_text_node",
            beforeNodes: [before],
            beforeConnections: [],
            afterNodes: [changed],
            afterConnections: [],
            result: { ok: true },
            now: T1,
        });
        const finished = canvasCollaborationAdapter.finishBatch({ state: recorded.state, runtime: recorded.runtime, nodes: recorded.nodes, connections: [], now: T1 });

        expect(finished.batch).toMatchObject({ status: "success", reversible: true, affectedNodeIds: [before.id], revision: 1 });
        expect(recorded.nodes[0].collaboration).toMatchObject({ lastEditedBy: "agent", lastAgentBatchId: "batch-1", revision: 1 });
        expect(finished.batch?.undo?.restoreNodes[0].full).toBeUndefined();

        const undone = canvasCollaborationAdapter.undoLatest(finished.state, recorded.nodes, [], T2);
        expect(undone.ok).toBeTrue();
        if (!undone.ok) return;
        expect(undone.state.revision).toBe(2);
        expect(undone.nodes[0].title).toBe("剧本");
        expect(undone.nodes[0].metadata?.content).toBe("旧内容");
        expect(undone.state.batches[0].undoneAt).toBe(T2);
    });

    test("批次后有人继续修改时禁止撤销以免覆盖", () => {
        const before = textNode();
        const started = canvasCollaborationAdapter.beginBatch({ state: canvasCollaborationAdapter.createState(T0), batchId: "batch-1", summary: "改标题", nodes: [before], connections: [], now: T0 });
        const after = { ...before, title: "Agent 标题" };
        const recorded = canvasCollaborationAdapter.recordAction({
            state: started.state,
            runtime: started.runtime,
            actionName: "update_node",
            beforeNodes: [before],
            beforeConnections: [],
            afterNodes: [after],
            afterConnections: [],
            result: { ok: true },
            now: T1,
        });
        const finished = canvasCollaborationAdapter.finishBatch({ state: recorded.state, runtime: recorded.runtime, nodes: recorded.nodes, connections: [], now: T1 });
        const human = canvasCollaborationAdapter.markHumanNodes(finished.state, [{ ...recorded.nodes[0], title: "人工最终标题" }], [before.id], T2);
        const undone = canvasCollaborationAdapter.undoLatest(human.state, human.nodes, [], T2);

        expect(undone.ok).toBeFalse();
        if (undone.ok) return;
        expect(undone.reason).toContain("为避免覆盖人工修改");
    });

    test("媒体生成批次保留历史但明确不可逆", () => {
        const started = canvasCollaborationAdapter.beginBatch({ state: canvasCollaborationAdapter.createState(T0), batchId: "batch-media", summary: "生成图片", nodes: [], connections: [], now: T0 });
        const generated: CanvasNodeData = { ...textNode("image-1"), type: CanvasNodeType.Image, metadata: { status: "loading" } };
        const recorded = canvasCollaborationAdapter.recordAction({
            state: started.state,
            runtime: started.runtime,
            actionName: "generate_image",
            beforeNodes: [],
            beforeConnections: [],
            afterNodes: [generated],
            afterConnections: [],
            result: { ok: true },
            now: T1,
        });
        const finished = canvasCollaborationAdapter.finishBatch({ state: recorded.state, runtime: recorded.runtime, nodes: recorded.nodes, connections: [], now: T1 });

        expect(finished.batch).toMatchObject({ status: "success", reversible: false });
        expect(finished.batch?.undo).toBeUndefined();
    });
});
