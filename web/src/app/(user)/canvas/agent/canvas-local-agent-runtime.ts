import { traceCanvasInput, traceCanvasTool } from "./canvas-context-trace";
import { openCanvasLocalAgent, type LocalAgentProvider, type LocalAgentMessage } from "@/services/canvas-local-agent";
import { applyAgentState, applyTaskResult, type RunCanvasAgentInput } from "./canvas-agent-runtime";
import { compactCodexCanvasContext } from "./canvas-codex-runtime";
import { buildCanvasAgentSkillBundle } from "./canvas-agent-skills";
import { CANVAS_AGENT_TOOLS, canvasAgentActionLabel, normalizeCanvasAgentAction } from "./canvas-agent-tools";

type Input = RunCanvasAgentInput & {
    provider: LocalAgentProvider; projectId: string; sessionId: string; resumeId?: string;
    onSession: (id: string) => void; onText: (text: string) => void; onInvalidate: () => void;
    onPermission: (title: string) => Promise<boolean>;
    onModel?: (model: string) => void;
};

export async function runCanvasLocalAgent(input: Input) {
    // CLI text transports must never silently discard user-provided image attachments.
    if (input.references.some((reference) => reference.dataUrl)) throw new Error("本机 Grok / Antigravity 的当前接入只支持文字与节点内容。请移除图片附件，或切换 Codex / API 后发送。");
    let state = input.initialState, text = "", settled = false, remoteId = input.resumeId;
    let connection: Awaited<ReturnType<typeof openCanvasLocalAgent>> | undefined;
    let reportedModel: string | undefined;
    const publishModel = (model: unknown) => {
        if (typeof model === "string" && model.trim() && model !== reportedModel) { reportedModel = model; input.onModel?.(model); }
    };
    let queue = Promise.resolve();
    let permissions = Promise.resolve();
    let readyTimer: ReturnType<typeof setTimeout> | undefined;
    let markReady!: () => void;
    const ready = new Promise<void>((resolve) => { markReady = resolve; });
    let textTimer: ReturnType<typeof setTimeout> | undefined;
    const publishText = (immediate = false) => {
        if (immediate) { clearTimeout(textTimer); textTimer = undefined; input.onText(text); }
        else if (!textTimer) textTimer = setTimeout(() => { textTimer = undefined; input.onText(text); }, 100);
    };
    let resolve!: () => void, reject!: (error: Error) => void;
    const completed = new Promise<void>((ok, error) => { resolve = ok; reject = error; });
    void completed.catch(() => {});
    const fail = (error: Error) => { if (!settled) { settled = true; input.onInvalidate(); reject(error); } };
    const active = () => { if (settled || input.signal?.aborted) throw new DOMException("已停止本机助手", "AbortError"); };
    const execute = async (message: LocalAgentMessage) => {
        active();
        const action = normalizeCanvasAgentAction(message.name, message.arguments || {}, message.id);
        if (!action) { await connection!.respond(message.id, { ok: false, message: "无效画布操作" }); return; }
        let result;
        try {
            input.onEvent?.({ status: "running", label: canvasAgentActionLabel(action) });
            const arrange = /整理|排列|排序|对齐|布局|排版|重新摆放/.test(input.userText) && !/(不要|别|无需|不用).{0,8}(整理|排列|排序|对齐|布局|排版|重新摆放)/.test(input.userText);
            if (action.name === "arrange_nodes" && !arrange) result = { ok: false, message: "用户未要求调整布局，保留原位置" };
            else if (action.name === "get_canvas_summary") {
                const context = input.getContext(state);
                result = { ok: true, project: context.project, agentState: state, selectedNodeIds: context.selectedNodeIds, nodes: context.nodes.map(({ id, title, type, status }) => ({ id, title, type, status })), note: "节点正文按需 get_node；索引可能不完整。" };
            } else result = await input.executeAction(action);
            active();
            if (result.ok) state = action.name === "set_agent_state" ? applyAgentState(state, action.arguments) : applyTaskResult(state, result);
            input.onCheckpoint?.({ state, protocolMessages: input.protocolMessages });
        } catch (error) {
            if (input.signal?.aborted || settled) throw error;
            result = { ok: false, message: error instanceof Error ? error.message : String(error) };
        }
        active();
        await connection!.respond(message.id, result);
        const trace = traceCanvasTool(action.name, result);
        if (trace) input.onContextTrace?.(trace);
    };
    try {
        input.onEvent?.({ status: "thinking", label: `正在连接本机 ${input.provider === "grok" ? "Grok" : "Antigravity"}` });
        connection = await openCanvasLocalAgent({
            provider: input.provider, projectId: input.projectId, sessionId: input.sessionId, resumeId: input.resumeId, signal: input.signal,
            tools: CANVAS_AGENT_TOOLS.map(({ function: tool }) => ({ name: tool.name, description: tool.description, inputSchema: tool.parameters })),
            onClose: fail,
            onMessage(message) {
                if (message.event === "canvas_tool") { queue = queue.then(() => execute(message)).catch(fail); return; }
                if (message.method === "session/request_permission") {
                    // Keep protocol permission separate from MCP execution: queueing it behind a
                    // tool waiting for approval would deadlock. Approval remains single-use.
                    permissions = permissions.then(async () => {
                        active();
                        const option = message.params?.options?.find((item: any) => item.kind === "allow_once");
                        const call = message.params?.toolCall;
                        const detail = String(call?.title || "Grok 请求执行工具") + (call?.rawInput ? "\n" + JSON.stringify(call.rawInput, null, 2).slice(0, 4000) : "");
                        const allow = option && await input.onPermission(detail);
                        active();
                        await connection!.permission(message.id, allow ? option.optionId : undefined);
                    }).catch(fail);
                    return;
                }
                if (message.method === "session/update" && (!remoteId || message.params?.sessionId === remoteId)) {
                    const update = message.params?.update;
                    publishModel(update?._meta?.modelId);
                    if (update?.sessionUpdate === "agent_message_chunk" && update.content?.type === "text") { text += update.content.text; publishText(); }
                }
                if (message.event === "init") { remoteId = message.conversation_id; input.onSession(remoteId!); markReady(); }
                if (message.event === "step_update" && message.step_update?.step_type === "agent_response") { text += message.step_update.text_delta || ""; publishText(); }
                if (message.event === "result") {
                    if (message.result?.status !== "SUCCESS") { fail(new Error(message.result?.error || "Antigravity 本轮未完成")); return; }
                    text = message.result.response || text;
                    publishText(true);
                    resolve();
                }
            },
        });
        active();
        const context = compactCodexCanvasContext(input.getContext(state));
        const bundle = buildCanvasAgentSkillBundle(state.phase, input.userText, context);
        const trace = input.onContextTrace ? await traceCanvasInput(context, bundle.sources) : undefined;
        const prompt = bundle.prompt
            + "\n你在画布侧栏工作。只通过 canvas MCP 工具操作真实画布；媒体和删除必须经过画布确认。不要直接执行终端、文件、外部生成服务。不可把节点链接当成已看过的图片。只有工具成功才报告完成。\n"
            + (input.references.length ? "本次引用节点：" + input.references.map((ref) => `${ref.label || ref.title} → ${ref.id}`).join("；") + "\n" : "")
            + "用户本轮消息：\n" + input.userText;
        if (input.provider === "grok") {
            const init = await connection.request("initialize");
            if (!init.authMethods?.some((item: any) => item.id === "cached_token")) throw new Error("请先在官方 Grok CLI 登录；当前未提供缓存登录入口");
            await connection.request("authenticate");
            active();
            if (remoteId && !init.agentCapabilities?.loadSession) throw new Error("本机 Grok 版本未提供会话恢复，请新建对话");
            const session = await connection.request(remoteId ? "session/load" : "session/new", remoteId ? { sessionId: remoteId } : {});
            publishModel(session.models?.currentModelId);
            remoteId ||= session.sessionId;
            input.onSession(remoteId!);
            active();
            const result = await connection.request("session/prompt", { sessionId: remoteId, prompt: [{ type: "text", text: prompt }] });
            if (trace) input.onContextTrace?.(trace);
            if (result.stopReason !== "end_turn") throw new Error(`Grok 本轮未正常完成：${result.stopReason}`);
        } else {
            const timeout = new Promise<never>((_, reject) => { readyTimer = setTimeout(() => reject(new Error("Antigravity 初始化超时，未发送模型消息")), 45000); });
            await Promise.race([ready, completed, timeout]);
            clearTimeout(readyTimer);
            active();
            await connection.send({ event: "user", message: { content: prompt } });
            if (trace) input.onContextTrace?.({ ...trace, label: "本轮已写入本机传输（模型接收未单独确认）" });
            await completed;
        }
        await queue;
        active();
        settled = true;
        publishText(true);
        return { reply: text || "本轮已结束，未返回文字。", state, protocolMessages: input.protocolMessages };
    } catch (error) { fail(error instanceof Error ? error : new Error(String(error))); throw error; }
    finally { clearTimeout(readyTimer); clearTimeout(textTimer); connection?.close(); }
}
