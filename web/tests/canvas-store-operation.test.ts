import { afterEach, describe, expect, test } from "bun:test";

import { DEFAULT_CANVAS_AGENT_PANEL, DEFAULT_CANVAS_SIDE_PANEL, useCanvasStore, type CanvasProject } from "../src/app/(user)/canvas/stores/use-canvas-store";
import { CANVAS_OPERATION_PROTOCOL_VERSION, createCanvasOperationState } from "../src/app/(user)/canvas/protocol/canvas-operation-protocol";
import { CanvasNodeType, type CanvasNodeData } from "../src/app/(user)/canvas/types";

const TIME = "2026-08-30T00:00:00.000Z";

function textNode(id: string, title: string): CanvasNodeData {
    return {
        id,
        type: CanvasNodeType.Text,
        title,
        position: { x: 0, y: 0 },
        width: 240,
        height: 160,
        metadata: { content: title },
    };
}

function emptyProject(): CanvasProject {
    return {
        id: "store-project",
        title: "Store 契约",
        createdAt: TIME,
        updatedAt: TIME,
        nodes: [],
        connections: [],
        chatSessions: [],
        activeChatId: null,
        agentConfig: null,
        autoTitlePending: false,
        backgroundMode: "lines",
        showImageInfo: false,
        viewport: { x: 0, y: 0, k: 1 },
        sidePanel: DEFAULT_CANVAS_SIDE_PANEL,
        agentPanel: DEFAULT_CANVAS_AGENT_PANEL,
        operationState: createCanvasOperationState({ nodes: [] }),
    };
}

afterEach(() => {
    useCanvasStore.setState({ projects: [], hydrated: false });
});

describe("Canvas store 公共操作入口", () => {
    test("UI updateProject 记为 human，Agent Bridge 记为 agent", () => {
        useCanvasStore.setState({ projects: [emptyProject()], hydrated: true });
        const humanNode = textNode("shared-node", "人工创建");

        useCanvasStore.getState().updateProject("store-project", { nodes: [humanNode] });
        const afterHuman = useCanvasStore.getState().openProject("store-project")!;
        const agent = useCanvasStore.getState().applyOperationBatch({
            protocolVersion: CANVAS_OPERATION_PROTOCOL_VERSION,
            actor: "agent",
            requestId: "bridge-update",
            projectId: "store-project",
            baseRevision: afterHuman.operationState.revision,
            timestamp: TIME,
            operations: [{ type: "node.update", nodeId: humanNode.id, patch: { title: "Agent 修改" } }],
        });

        expect(agent?.result.ok).toBe(true);
        expect(agent?.project.nodes[0].title).toBe("Agent 修改");
        expect(agent?.project.operationState.audit.map((entry) => entry.batch.actor)).toEqual(["human", "agent"]);
        expect(useCanvasStore.getState().openProject("store-project")?.operationState.revision).toBe(2);
    });
});
