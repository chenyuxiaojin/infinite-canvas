// Frozen installed first ACK revision for the latency comparison; NOT production code.
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

export type PtySpawnOptions = {
    session_id: string;
    shell?: string;
    cwd?: string;
    cols?: number;
    rows?: number;
};

export type CanvasProjectWorkspace = {
    projectDirectory: string;
    configured: boolean;
    source: "saved_binding" | "matched_title" | "selected_folder" | "workflow_root";
    configurationError?: string | null;
    agentCommand?: string | null;
};

export function isTerminalAvailable() {
    return isTauri();
}

const OUTPUT_BUDGET = 256 * 1024;
const outputSessions = new Map<string, { dispose: () => void }>();

export async function spawnPty(
    options: PtySpawnOptions,
    onData: (data: Uint8Array, consumed: () => void) => void,
    onExit: () => void,
    onError: (message: string) => void,
): Promise<boolean> {
    if (!isTauri()) return false;
    if (outputSessions.has(options.session_id)) throw new Error("终端会话已存在");
    let active = true;
    let spawning = true;
    let eof = false;
    let received = 0;
    let consumed = 0;
    let acknowledgements = Promise.resolve();
    const report = (error: unknown) => {
        // UI callbacks must never escape into the ordered Tauri Channel.
        try { onError(error instanceof Error ? error.message : String(error)); } catch { /* preserve transport cleanup */ }
    };
    const dispose = () => {
        if (!active) return;
        active = false;
        onOutput.onmessage = () => {};
        // Tauri 2's Channel has no public dispose API. This is the same pinned
        // callback deregistration used by its private cleanupCallback method;
        // also needed when invoke rejects before Rust can send Channel end.
        const internals = (window as unknown as { __TAURI_INTERNALS__: { unregisterCallback: (id: number) => void } }).__TAURI_INTERNALS__;
        internals.unregisterCallback(onOutput.id);
        // Keep the ID reserved until a late spawn settles, so its compensating
        // terminate cannot kill a new session reusing the same ID.
        if (!spawning && outputSessions.get(options.session_id)?.dispose === dispose) outputSessions.delete(options.session_id);
    };
    const fail = (error: unknown) => {
        if (!active) return;
        dispose();
        report(error);
        void invoke<void>("pty_terminate", { sessionId: options.session_id }).catch((error) => report(`终端结束失败：${String(error)}`));
    };
    const onOutput = new Channel<ArrayBuffer | { error: string }>((buffer) => {
        if (!active) return;
        try {
            if (!(buffer instanceof ArrayBuffer)) {
                throw new Error(buffer && typeof buffer.error === "string" ? buffer.error : "终端输出协议无效");
            }
            if (eof) throw new Error("终端退出后仍收到输出");
            if (buffer.byteLength === 0) {
                eof = true;
                // Rust only emits EOF after consumption. Also wait for ACK RPC
                // promises so a rejected final ACK cannot be shown as success.
                if (consumed !== received) throw new Error("终端在输出消费完成前退出");
                void acknowledgements.then(() => {
                    if (!active) return;
                    dispose();
                    try { onExit(); } catch (error) { report(error); }
                }).catch(fail);
                return;
            }
            // Rust may send the newly credited packet before invoke's ACK
            // promise resolves. Count actual consumption here, not RPC latency.
            if (buffer.byteLength > 16 * 1024 || received - consumed + buffer.byteLength > OUTPUT_BUDGET) throw new Error("终端输出超出未消费预算");
            received += buffer.byteLength;
            const end = received;
            let completed = false;
            onData(new Uint8Array(buffer), () => {
                if (!active || completed) return;
                completed = true;
                try {
                    if (consumed + buffer.byteLength !== end) throw new Error("终端消费回调顺序错误");
                    consumed = end;
                    acknowledgements = acknowledgements.then(async () => {
                        if (!active) return;
                        await invoke<void>("pty_ack", { sessionId: options.session_id, consumed: end });
                    });
                    void acknowledgements.catch(fail);
                } catch (error) { fail(error); }
            });
        } catch (error) { fail(error); }
    });
    outputSessions.set(options.session_id, { dispose });
    try {
        const result = await invoke<boolean>("pty_spawn", { options, onOutput });
        // terminate may have won before the Rust spawn command was dispatched.
        if (!active) await invoke<void>("pty_terminate", { sessionId: options.session_id });
        if (!result) dispose();
        return result;
    } catch (error) {
        fail(error);
        dispose();
        throw error;
    } finally {
        spawning = false;
        if (!active && outputSessions.get(options.session_id)?.dispose === dispose) outputSessions.delete(options.session_id);
    }
}

export async function terminatePty(sessionId: string): Promise<void> {
    if (!isTauri()) return;
    outputSessions.get(sessionId)?.dispose();
    return invoke<void>("pty_terminate", { sessionId });
}
