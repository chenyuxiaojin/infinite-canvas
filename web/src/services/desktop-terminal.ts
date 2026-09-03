import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type PtyOutputPayload = {
    session_id: string;
    data: string;
};

export type PtyExitPayload = {
    session_id: string;
    exit_code?: number;
};

export type PtySpawnOptions = {
    session_id: string;
    shell?: string;
    cwd?: string;
    cols?: number;
    rows?: number;
};

export function isTerminalAvailable() {
    return isTauri();
}

export async function spawnPty(options: PtySpawnOptions): Promise<boolean> {
    if (!isTauri()) return false;
    return invoke<boolean>("pty_spawn", { options });
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
    return invoke<void>("pty_terminate", { sessionId });
}

export async function onPtyData(callback: (payload: PtyOutputPayload) => void): Promise<UnlistenFn> {
    return listen<PtyOutputPayload>("pty_data", (event) => callback(event.payload));
}

export async function onPtyExit(callback: (payload: PtyExitPayload) => void): Promise<UnlistenFn> {
    return listen<PtyExitPayload>("pty_exit", (event) => callback(event.payload));
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
