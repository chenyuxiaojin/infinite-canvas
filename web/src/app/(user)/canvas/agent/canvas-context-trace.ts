import type { CanvasAgentContext } from "./canvas-agent-context";

export type CanvasContextTrace = {
    kind: "input" | "tool";
    label: string;
    nodes: Array<{ id: string; title: string; detail: "body" | "index" | "image" }>;
    sources?: Array<{ source: string; sha256: string }>;
};

// Fingerprint only the already-selected embedded instructions. No file reads,
// additional SOP injection or duplicated body text in persistent chat history.
export async function traceCanvasInput(context: CanvasAgentContext, sources: Array<{ source: string; content: string }>, imageIds: string[] = []): Promise<CanvasContextTrace> {
    return {
        kind: "input", label: "本轮传输已确认",
        nodes: [...context.nodes.map((node): CanvasContextTrace["nodes"][number] => ({ id: node.id, title: node.title, detail: imageIds.includes(node.id) ? "image" : node.text || node.prompt ? "body" : "index" })), ...imageIds.filter((id) => !context.nodes.some((node) => node.id === id)).map((id): CanvasContextTrace["nodes"][number] => ({ id, title: id, detail: "image" }))],
        sources: await Promise.all(sources.map(async ({ source, content }) => ({ source, sha256: Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(content))), (byte) => byte.toString(16).padStart(2, "0")).join("") }))),
    };
}

export function traceCanvasTool(name: string, result: Record<string, unknown>): CanvasContextTrace | undefined {
    if (!name.startsWith("get_") || !result.ok) return;
    const nodes: CanvasContextTrace["nodes"] = [];
    const add = (value: unknown) => {
        if (!value || typeof value !== "object") return;
        const node = value as Record<string, unknown>;
        if (typeof node.id === "string") nodes.push({ id: node.id, title: String(node.title || node.id), detail: name === "get_canvas_summary" ? "index" : "body" });
    };
    add(result.node);
    for (const key of ["nodes", "upstream", "downstream"]) if (Array.isArray(result[key])) result[key].forEach(add);
    return { kind: "tool", label: `工具返回：${name}`, nodes };
}
