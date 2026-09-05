import { traceCanvasInput, traceCanvasTool } from "./canvas-context-trace";
import { openCanvasCodex, type CanvasCodexUsage, type CodexMessage } from "@/services/canvas-codex";
import type { CanvasAgentContext } from "./canvas-agent-context";
import { applyAgentState, applyTaskResult, type RunCanvasAgentInput } from "./canvas-agent-runtime";
import { buildCanvasAgentSkillBundle } from "./canvas-agent-skills";
import { CANVAS_AGENT_ACTION_NAMES, CANVAS_AGENT_TOOLS, canvasAgentActionLabel, normalizeCanvasAgentAction } from "./canvas-agent-tools";

export function compactCodexCanvasContext(context: CanvasAgentContext): CanvasAgentContext {
    const relevant = new Set([...context.selectedNodeIds, ...context.agentState.approvedNodeIds, ...context.agentState.referenceNodeIds]);
    const nodes = context.nodes.filter((node) => relevant.has(node.id)).slice(0, 16);
    const ids = new Set(nodes.map((node) => node.id));
    return { ...context, nodes, connections: context.connections.filter((edge) => ids.has(edge.fromNodeId) && ids.has(edge.toNodeId)), tasks: context.tasks.filter((task) => ids.has(task.nodeId)) };
}

type CodexInput = RunCanvasAgentInput & {
    projectId: string;
    sessionId: string;
    threadId?: string;
    onCodexUpdate: (patch: { codexThreadId?: string; codexModel?: string; codexUsage?: CanvasCodexUsage; codexCompaction?: string }) => void;
    onText: (text: string) => void;
    onInvalidate: () => void;
};

export async function runCanvasCodex(input: CodexInput) {
    let state = input.initialState;
    let threadId = input.threadId;
    let reply = "";
    let finalReply = "";
    let settled = false;
    let toolQueue = Promise.resolve();
    const toolResults = new Map<string, unknown>();
    const texts = new Map<string, string>();
    let textTimer: ReturnType<typeof setTimeout> | undefined;
    const publishText = (immediate = false) => {
        if (immediate) {
            clearTimeout(textTimer);
            textTimer = undefined;
            input.onText(finalReply || reply);
        } else if (!textTimer) {
            textTimer = setTimeout(() => { textTimer = undefined; input.onText(finalReply || reply); }, 100);
        }
    };
    let resolveTurn!: () => void;
    let rejectTurn!: (error: Error) => void;
    const completed = new Promise<void>((resolve, reject) => { resolveTurn = resolve; rejectTurn = reject; });
    // The stream can close while initialize/start is still awaiting its RPC.
    void completed.catch(() => {});
    const fail = (error: Error) => { if (!settled) { settled = true; input.onInvalidate(); rejectTurn(error); } };
    const assertActive = () => { if (input.signal?.aborted || settled) throw new DOMException("已停止 Codex", "AbortError"); };
    let connection: Awaited<ReturnType<typeof openCanvasCodex>> | undefined;
    const checkpoint = () => input.onCheckpoint?.({ state, protocolMessages: input.protocolMessages });

    const executeTool = async (message: CodexMessage) => {
        if (!connection || message.id === undefined) return;
        const params = message.params || {};
        const callId = String(params.callId || message.id);
        if (toolResults.has(callId)) { await connection.respond(message.id, toolResults.get(callId)); return; }
        assertActive();
        let result;
        if (params.threadId !== threadId || !CANVAS_AGENT_ACTION_NAMES.includes(params.tool)) {
            result = { ok: false, code: "tool_not_allowed", message: "只能操作当前画布提供的工具" };
        } else {
            const action = normalizeCanvasAgentAction(params.tool, params.arguments, callId);
            if (!action) throw new Error("Codex 返回了无效画布操作");
            input.onEvent?.({ status: "running", label: canvasAgentActionLabel(action) });
            const arrangeRequested = /整理|排列|排序|对齐|布局|排版|重新摆放/.test(input.userText) && !/(不要|别|无需|不用).{0,8}(整理|排列|排序|对齐|布局|排版|重新摆放)/.test(input.userText);
            try {
                if (action.name === "arrange_nodes" && !arrangeRequested) {
                    result = { ok: false, code: "arrange_not_requested", message: "用户未要求整理布局，保留原位置" };
                } else if (action.name === "get_canvas_summary") {
                    const context = input.getContext(state);
                    result = { ok: true, project: context.project, agentState: state, selectedNodeIds: context.selectedNodeIds, nodes: context.nodes.map(({ id, title, type, status }) => ({ id, title, type, status })), note: "这里只列索引；需要正文请用 get_node。索引最多 120 个，未列出不代表不存在。" };
                } else {
                    result = await input.executeAction(action);
                }
                if (result.ok) state = action.name === "set_agent_state" ? applyAgentState(state, action.arguments) : applyTaskResult(state, result);
                checkpoint();
            } catch (error) {
                if (input.signal?.aborted) throw error;
                result = { ok: false, code: "tool_execution_failed", message: error instanceof Error ? error.message : String(error) };
            }
        }
        assertActive();
        const response = { success: result.ok, contentItems: [{ type: "inputText", text: JSON.stringify(result) }] };
        toolResults.set(callId, response);
        await connection.respond(message.id, response);
        const trace = traceCanvasTool(String(params.tool), result);
        if (trace) input.onContextTrace?.(trace);
        input.onEvent?.({ status: "thinking", label: "Codex 正在根据画布结果继续" });
    };

    try {
        input.onEvent?.({ status: "thinking", label: "正在连接本机 Codex" });
        connection = await openCanvasCodex({
            projectId: input.projectId, sessionId: input.sessionId, signal: input.signal, onClose: fail,
            onMessage: (message) => {
                const params = message.params || {};
                if (message.method === "item/tool/call") { toolQueue = toolQueue.then(() => executeTool(message)).catch(fail); return; }
                if (params.threadId && threadId && params.threadId !== threadId) return;
                if (message.method === "item/agentMessage/delta") {
                    const id = String(params.itemId);
                    texts.set(id, (texts.get(id) || "") + (params.delta || ""));
                    reply = [...texts.values()].join("\n\n");
                    publishText();
                }
                if (message.method === "item/completed" && params.item?.type === "agentMessage") {
                    texts.set(params.item.id, params.item.text || texts.get(params.item.id) || "");
                    if (params.item.phase === "final_answer") finalReply = params.item.text;
                    reply = [...texts.values()].join("\n\n");
                    publishText(true);
                }
                if (message.method === "thread/tokenUsage/updated" && params.tokenUsage?.last) {
                    const usage = params.tokenUsage;
                    input.onCodexUpdate({ codexUsage: { inputTokens: usage.last.inputTokens, outputTokens: usage.last.outputTokens, contextWindow: usage.modelContextWindow ?? null } });
                }
                if ((message.method === "item/started" || message.method === "item/completed") && params.item?.type === "contextCompaction") {
                    const label = message.method === "item/started" ? "正在压缩早期上下文" : "早期上下文已压缩为摘要";
                    input.onCodexUpdate({ codexCompaction: label });
                    input.onEvent?.({ status: "thinking", label });
                }
                if (message.method === "warning" && params.message) input.onEvent?.({ status: "thinking", label: params.message });
                if (message.method === "turn/completed") {
                    if (params.turn?.status === "failed") fail(new Error(params.turn.error?.message || "Codex 本轮失败"));
                    else if (params.turn?.status === "interrupted") fail(new DOMException("已停止 Codex", "AbortError"));
                    else { settled = true; resolveTurn(); }
                }
            },
        });
        await connection.request("initialize");
        await connection.notify("initialized");
        const account = await connection.request("account/read");
        if (account.accountType !== "chatgpt") throw new Error("请先在官方 Codex 登录 ChatGPT；画布试接不使用 API 密钥");
        await connection.request("config/read");
        assertActive();
        const context = compactCodexCanvasContext(input.getContext(state));
        const bundle = buildCanvasAgentSkillBundle(state.phase, input.userText, context);
        const trace = input.onContextTrace ? await traceCanvasInput(context, bundle.sources, input.references.filter((ref) => ref.dataUrl).map((ref) => ref.id)) : undefined;
        const instructions = bundle.prompt
            + "\n\n你运行在画布聊天栏，不是编程终端。只能使用提供的画布工具，不运行 Shell、不读写项目文件、不启用其他插件或 Agent。需要用户选择时直接用普通文字提问。画布快照只含部分相关节点；其他正文按需 get_node，整体目录用 get_canvas_summary。所有生成和删除以画布确认结果为准，取消后不可自动重试。";
        const thread = await connection.request(threadId ? "thread/resume" : "thread/start", {
            ...(threadId ? { threadId } : { dynamicTools: CANVAS_AGENT_TOOLS.map(({ function: tool }) => ({ type: "function", name: tool.name, description: tool.description, inputSchema: tool.parameters })) }),
            baseInstructions: "你是小陈的画布中的影视创作助手。使用中文，简洁回答。只依据真实画布工具结果汇报完成状态。",
            developerInstructions: instructions,
        });
        threadId = thread.thread.id;
        input.onCodexUpdate({ codexThreadId: threadId, codexModel: thread.model });
        assertActive();
        const referenceText = input.references.map((reference) => `${reference.label || reference.title} → 节点 ${reference.id}`).join("；");
        const images = input.references.filter((reference) => /^data:image\//.test(reference.dataUrl || "") || /^https?:\/\//.test(reference.dataUrl || ""));
        await connection.request("turn/start", {
            threadId,
            input: [{ type: "text", text: input.userText + (referenceText ? "\n本次明确引用：" + referenceText : "") + (images.length ? "\n附图顺序：" + images.map((reference, index) => `${index + 1} = 节点 ${reference.id}`).join("；") : "") }, ...images.map((reference) => ({ type: "image", url: reference.dataUrl }))],
        });
        if (trace) input.onContextTrace?.(trace);
        await completed;
        await toolQueue;
        return { reply: finalReply || reply || "Codex 本轮已结束，未返回文字。", state, protocolMessages: input.protocolMessages };
    } finally { clearTimeout(textTimer); connection?.close(); }
}
