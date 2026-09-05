import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { CanvasNodeType, type CanvasAgentChangeBatch, type CanvasNodeData } from "../types";
import { CanvasCollaborationHistory, CanvasCollaborationStatus } from "./canvas-collaboration-status";

const node: CanvasNodeData = {
    id: "node-1",
    type: CanvasNodeType.Text,
    title: "第一幕",
    position: { x: 0, y: 0 },
    width: 320,
    height: 240,
};

const batch: CanvasAgentChangeBatch = {
    id: "batch-1",
    actor: "Canvas Agent",
    startedAt: "2026-08-30T01:00:00.000Z",
    completedAt: "2026-08-30T01:00:01.000Z",
    summary: "改写第一幕",
    actionNames: ["update_text_node"],
    affectedNodeIds: [node.id],
    affectedNodeTitles: { [node.id]: node.title },
    baseRevision: 3,
    revision: 4,
    status: "success",
    reversible: true,
    canUndoNow: true,
};

describe("CanvasCollaborationStatus", () => {
    test("状态按钮同时暴露文字、图标语义和 revision", () => {
        const html = renderToStaticMarkup(
            <CanvasCollaborationStatus
                collaboration={{ revision: 4, batches: [batch], status: { state: "running", message: "正在更新节点", batchId: batch.id, affectedNodeIds: [node.id], updatedAt: batch.completedAt } }}
                nodes={[node]}
                onUndoLatest={() => undefined}
            />,
        );

        expect(html).toContain("Agent 正在执行");
        expect(html).toContain("rev 4");
        expect(html).toContain('aria-live="polite"');
        expect(html).toContain("画布 revision 4");
    });

    test("批次历史显示操作者、摘要、影响节点并提供零付费入口", () => {
        const html = renderToStaticMarkup(
            <CanvasCollaborationHistory
                collaboration={{ revision: 4, batches: [batch], status: { state: "success", message: "完成", batchId: batch.id, affectedNodeIds: [node.id], updatedAt: batch.completedAt } }}
                nodes={[node]}
                onUndoLatest={() => undefined}
                onRunDemo={() => undefined}
            />,
        );

        expect(html).toContain("Canvas Agent");
        expect(html).toContain("改写第一幕");
        expect(html).toContain("第一幕");
        expect(html).toContain("撤销批次");
        expect(html).toContain("运行零付费本地协作演示");
    });

    test("后续人工 revision 会显示撤销保护而不是继续宣称可撤销", () => {
        const html = renderToStaticMarkup(
            <CanvasCollaborationHistory
                collaboration={{ revision: 5, batches: [{ ...batch, canUndoNow: false }], status: { state: "success", message: "完成", batchId: batch.id, affectedNodeIds: [node.id], updatedAt: batch.completedAt } }}
                nodes={[]}
                onUndoLatest={() => undefined}
            />,
        );

        expect(html).toContain("完成 · 已有后续修改");
        expect(html).toContain("第一幕");
        expect(html).toContain("disabled");
    });
});
