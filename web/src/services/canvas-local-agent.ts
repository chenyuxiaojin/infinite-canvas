import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

export type LocalAgentProvider = "grok" | "antigravity";
export type LocalAgentMessage = Record<string, any>;
export async function openCanvasLocalAgent(input: {
    provider: LocalAgentProvider; projectId: string; sessionId: string; resumeId?: string;
    tools: unknown[]; signal?: AbortSignal; onMessage: (message: LocalAgentMessage) => void; onClose: (error: Error) => void;
}) {
    if (!isTauri()) throw new Error("本机助手仅在小陈的画布桌面版可用");
    const connectionId = crypto.randomUUID();
    const channel = new Channel<LocalAgentMessage>();
    let closed = false, sequence = 0;
    const pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
    const close = () => {
        if (closed) return;
        closed = true;
        input.signal?.removeEventListener("abort", abort);
        for (const item of pending.values()) { clearTimeout(item.timer); item.reject(new Error("本机助手已停止")); }
        pending.clear();
        channel.onmessage = () => {};
        (window as unknown as { __TAURI_INTERNALS__?: { unregisterCallback: (id: number) => void } }).__TAURI_INTERNALS__?.unregisterCallback(channel.id);
        void invoke("canvas_local_agent_close", { connectionId }).catch(() => {});
    };
    const fail = (error: Error) => { close(); input.onClose(error); };
    const abort = () => fail(new DOMException("已停止本机助手", "AbortError"));
    const send = (message: LocalAgentMessage) => {
        if (closed) return Promise.reject(new Error("本机助手已停止"));
        return invoke<void>("canvas_local_agent_send", { connectionId, message });
    };
    channel.onmessage = (message) => {
        if (closed) return;
        if (message.event === "error") { fail(new Error(message.message || "本机助手连接失败")); return; }
        if (typeof message.id === "number" && !message.method && !message.event) {
            const item = pending.get(message.id);
            if (item) { pending.delete(message.id); clearTimeout(item.timer); message.error ? item.reject(new Error(message.error.message)) : item.resolve(message.result); }
            return;
        }
        try { input.onMessage(message); } catch (error) { fail(error instanceof Error ? error : new Error(String(error))); }
    };
    if (input.signal?.aborted) throw new DOMException("已停止本机助手", "AbortError");
    input.signal?.addEventListener("abort", abort, { once: true });
    try {
        await invoke("canvas_local_agent_open", { connectionId, provider: input.provider, projectId: input.projectId, sessionId: input.sessionId, resumeId: input.resumeId, tools: input.tools, onEvent: channel });
        if (closed) { await invoke("canvas_local_agent_close", { connectionId }); throw new DOMException("已停止本机助手", "AbortError"); }
    } catch (error) { close(); throw error; }
    return {
        close, send,
        respond: (id: string, result: unknown) => invoke<void>("canvas_local_agent_respond", { connectionId, id, result }),
        permission: (id: string | number, optionId?: string) => invoke<void>("canvas_local_agent_permission", { connectionId, id, optionId }),
        request: (method: string, params: Record<string, unknown> = {}): Promise<any> => new Promise((resolve, reject) => {
            if (closed) { reject(new Error("本机助手已停止")); return; }
            const id = ++sequence;
            const timer = setTimeout(() => { pending.delete(id); reject(new Error("本机助手等待超时，未自动重试")); }, method === "session/prompt" ? 20 * 60_000 : 45_000);
            pending.set(id, { resolve, reject, timer });
            send({ id, method, params }).catch((error) => { pending.delete(id); clearTimeout(timer); reject(error); });
        }),
    };
}
