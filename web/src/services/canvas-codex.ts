import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

export type CodexMessage = { id?: string | number; method?: string; params?: Record<string, any>; result?: any; error?: { message: string } };
export type CanvasCodexUsage = { inputTokens: number; outputTokens: number; contextWindow: number | null };

export function codexContextPercent(usage?: CanvasCodexUsage) {
    if (!usage || !usage.contextWindow || usage.contextWindow <= 0) return null;
    return Math.min(100, Math.max(0, Math.round((usage.inputTokens + usage.outputTokens) / usage.contextWindow * 100)));
}

export async function openCanvasCodex(input: {
    projectId: string;
    sessionId: string;
    signal?: AbortSignal;
    onMessage: (message: CodexMessage) => void;
    onClose: (error: Error) => void;
}) {
    if (!isTauri()) throw new Error("本机 Codex 仅在小陈的画布桌面版可用");
    const connectionId = crypto.randomUUID();
    let closed = false;
    let sequence = 0;
    const pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
    const channel = new Channel<CodexMessage>();
    const send = (message: CodexMessage) => invoke<void>("canvas_codex_send", { connectionId, message });
    const close = (error = new Error("Codex 连接已关闭")) => {
        if (closed) return;
        closed = true;
        input.signal?.removeEventListener("abort", abort);
        for (const item of pending.values()) { clearTimeout(item.timer); item.reject(error); }
        pending.clear();
        channel.onmessage = () => {};
        const internals = (window as unknown as { __TAURI_INTERNALS__?: { unregisterCallback: (id: number) => void } }).__TAURI_INTERNALS__;
        internals?.unregisterCallback(channel.id);
        void invoke("canvas_codex_close", { connectionId }).catch(() => {});
    };
    const abort = () => {
        const error = new DOMException("已停止 Codex", "AbortError");
        close(error);
        input.onClose(error);
    };
    channel.onmessage = (message) => {
        if (closed) return;
        if (typeof message.id === "number" && !message.method) {
            const item = pending.get(message.id);
            if (!item) return;
            pending.delete(message.id);
            clearTimeout(item.timer);
            message.error ? item.reject(new Error(message.error.message)) : item.resolve(message.result);
            return;
        }
        if (message.method === "canvas/closed") {
            const error = new Error(message.params?.message || "Codex 连接意外结束");
            close(error);
            input.onClose(error);
            return;
        }
        try { input.onMessage(message); }
        catch (error) { const reason = error instanceof Error ? error : new Error(String(error)); close(reason); input.onClose(reason); }
    };
    if (input.signal?.aborted) { close(); throw new DOMException("已停止 Codex", "AbortError"); }
    input.signal?.addEventListener("abort", abort, { once: true });
    try {
        await invoke("canvas_codex_open", { connectionId, projectId: input.projectId, sessionId: input.sessionId, onEvent: channel });
        if (closed) {
            await invoke("canvas_codex_close", { connectionId });
            throw new DOMException("已停止 Codex", "AbortError");
        }
    } catch (error) { close(); throw error; }
    return {
        close,
        notify: (method: string) => send({ method, params: {} }),
        respond: (id: string | number, result: unknown) => send({ id, result }),
        request: (method: string, params: Record<string, unknown> = {}): Promise<any> => {
            if (closed) return Promise.reject(new Error("Codex 连接已关闭"));
            const id = ++sequence;
            return new Promise((resolve, reject) => {
                const timer = setTimeout(() => {
                    pending.delete(id);
                    reject(new Error("Codex 接口等待超时；未自动重试，请检查本机 Codex 状态"));
                }, 45_000);
                pending.set(id, { resolve, reject, timer });
                send({ id, method, params }).catch((error) => { clearTimeout(timer); pending.delete(id); reject(error); });
            });
        },
    };
}
