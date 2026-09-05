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

export type CanvasBindingInfo = { projectId: string; state: "bound" | "unbound" | "duplicate" | "invalid"; directories: string[]; message: string };
export function inspectCanvasProjectBindings(projectIds: string[]) {
    return invoke<CanvasBindingInfo[]>("inspect_canvas_project_bindings", { projectIds });
}

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
    let acknowledged = 0;
    let ackInFlight = false;
    let ackScheduled = false;
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
    const finishExit = () => {
        if (!active || !eof || ackInFlight || ackScheduled || acknowledged !== consumed) return;
        dispose();
        try { onExit(); } catch (error) { report(error); }
    };
    const scheduleAck = () => {
        if (!active || ackScheduled || ackInFlight || consumed === acknowledged) return;
        ackScheduled = true;
        // xterm consumes many small PTY packets in one parser turn. Confirm the
        // latest consumed boundary once per turn, not one serial RPC per packet.
        // While an ACK is in flight, retain only the latest cumulative boundary.
        queueMicrotask(() => {
            ackScheduled = false;
            if (!active || ackInFlight || consumed === acknowledged) return;
            const end = consumed;
            ackInFlight = true;
            void Promise.resolve().then(() => invoke<void>("pty_ack", { sessionId: options.session_id, consumed: end })).then(() => {
                acknowledged = end;
                ackInFlight = false;
                if (!active) return;
                scheduleAck();
                finishExit();
            }).catch(fail);
        });
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
                // Rust only emits EOF after consumption. Its final ACK can
                // still be in flight: never show success before it resolves.
                if (consumed !== received) throw new Error("终端在输出消费完成前退出");
                finishExit();
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
                    scheduleAck();
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

export async function writePty(sessionId: string, data: string): Promise<void> {
    if (!isTauri()) return;
    return invoke<void>("pty_write", { sessionId, data });
}

export async function resizePty(sessionId: string, cols: number, rows: number): Promise<void> {
    if (!isTauri()) return;
    return invoke<void>("pty_resize", { sessionId, cols, rows });
}

export async function terminatePty(sessionId: string): Promise<void> {
    if (!isTauri()) return;
    outputSessions.get(sessionId)?.dispose();
    return invoke<void>("pty_terminate", { sessionId });
}

export async function resolveCanvasProjectWorkspace(projectId?: string, projectTitle?: string): Promise<CanvasProjectWorkspace> {
    if (!isTauri() || !projectId) {
        return {
            projectDirectory: resolveCaseProjectCwd(projectTitle, projectId),
            configured: false,
            source: "workflow_root",
        };
    }
    return invoke<CanvasProjectWorkspace>("resolve_canvas_project_workspace", {
        projectId,
        projectTitle: projectTitle || "未命名片子",
    });
}

export async function selectFilmDirectory(): Promise<string | null> {
    if (!isTauri()) return null;
    return invoke<string | null>("select_film_directory");
}

export async function bindCanvasProjectDirectory(projectId: string, projectTitle: string, projectDirectory: string): Promise<CanvasProjectWorkspace> {
    if (!isTauri()) {
        return { projectDirectory, configured: false, source: "selected_folder" };
    }
    return invoke<CanvasProjectWorkspace>("bind_canvas_project_directory", {
        projectId,
        projectTitle,
        projectDirectory,
    });
}

/**
 * 根据工程标题与 ID 解析 Option A: 自动进入对应的片子案例目录
 */
export function resolveCaseProjectCwd(projectTitle?: string, projectId?: string): string {
    const basePipelineDir = "/Users/chenhuajin/项目/视频制作台/AI编导";
    const title = (projectTitle || "").toLowerCase();
    const pid = (projectId || "").toLowerCase();

    if (title.includes("案例4") || title.includes("克兰奇") || pid.includes("case4") || pid.includes("clench")) {
        return `${basePipelineDir}/案例4-克兰奇杀妻案`;
    }
    if (title.includes("案例3") || title.includes("国运") || pid.includes("case3")) {
        return `${basePipelineDir}/案例3-国运末世`;
    }
    if (title.includes("案例2") || title.includes("美甲") || pid.includes("case2") || pid.includes("mjs")) {
        return `${basePipelineDir}/案例2-美甲师日常`;
    }
    if (title.includes("案例1") || title.includes("飞机稿") || pid.includes("case1")) {
        return `${basePipelineDir}/案例1-飞机稿`;
    }
    return basePipelineDir;
}
