import { invoke, isTauri } from "@tauri-apps/api/core";

export type RuntimeStatus = "available" | "unavailable" | "not_installed" | "not_running" | "permission_missing" | "incompatible" | "ready" | "discovered" | "model_missing" | "error";

export type DesktopRuntimeReport = {
    transport: "tauri_ipc";
    ffmpeg: {
        status: "available" | "unavailable" | "error";
        diagnostic: string;
        tools: Array<{ name: string; version_line: string }>;
    };
    connectors: Array<{
        provider: "eagle" | "davinci_resolve";
        status: RuntimeStatus;
        diagnostic: string;
        capabilities: string[];
        facts: Record<string, unknown>;
    }>;
    audio: {
        status: "ok" | "service_only" | "error";
        diagnostic: string;
        providers: Array<{
            provider: "index_tts_25" | "vox_cpm_2";
            display_name: string;
            status: RuntimeStatus;
            capabilities: {
                speech_synthesis: boolean;
                voice_design: boolean;
                reference_audio: boolean;
                output_formats: string[];
            };
            installation_found: boolean;
            models_complete: boolean;
            runtime_version?: string;
            runtime_compatible: boolean;
            service_status: "ready" | "not_running" | "unexpected_response" | "error" | "not_checked";
            service_identity_confirmed: boolean;
            end_to_end: "not_run" | "passed" | "failed";
        }>;
    };
};

export type DesktopTaskMedia = {
    task_id: string;
    mime_type: "video/mp4";
    file_name: string;
    sha256: string;
    bytes: number[];
};

export type DesktopTaskSnapshot = {
    id: string;
    status: "queued" | "running" | "succeeded" | "failed" | "cancelled";
    action: "generate_test_clip" | "transcode_to_mp4" | "verify_media";
    result?: {
        type: "media_created" | "media_verified";
        sha256: string;
        probe: {
            duration_ms?: number;
            streams: Array<{
                index: number;
                codec_type: string;
                codec_name?: string;
            }>;
        };
    };
    error?: {
        code: string;
        message: string;
        retryable: boolean;
        side_effects_may_exist: boolean;
    };
};

export function isDesktopRuntime() {
    return isTauri();
}

export function probeDesktopRuntime() {
    return invoke<DesktopRuntimeReport>("probe_desktop_runtime");
}

export function generateDesktopTestClip() {
    return invoke<{ task_id: string; duplicate: boolean }>("generate_desktop_test_clip");
}

export function generateCanvasTestClip(projectId: string) {
    return invoke<{ task_id: string; duplicate: boolean }>("generate_canvas_test_clip", { projectId });
}

export function fetchDesktopTaskStatus(taskId: string) {
    return invoke<DesktopTaskSnapshot>("desktop_task_status", { taskId });
}

export function fetchDesktopTaskMedia(taskId: string) {
    return invoke<DesktopTaskMedia>("desktop_task_media", { taskId });
}

export function cancelDesktopTask(taskId: string) {
    return invoke<boolean>("cancel_desktop_task", { taskId });
}

export function listDesktopCanvasProjects<T>() {
    return invoke<T[]>("desktop_canvas_projects");
}

export async function loadDesktopCanvasProjects<T>() {
    const projectIds = await invoke<string[]>("desktop_canvas_project_ids");
    return Promise.all(
        projectIds.map(async (projectId) => {
            const document = await invoke<{ project: T; revision: string }>("desktop_canvas_document", { projectId });
            return { ...document.project, __desktopRevision: document.revision };
        }),
    );
}

export async function saveDesktopCanvasProject<T>(project: T) {
    const { __desktopRevision, ...content } = project as T & { __desktopRevision?: string };
    const document = await invoke<{ project: T; revision: string }>("save_desktop_canvas_project", { project: content, expectedRevision: __desktopRevision || "" });
    return { ...document.project, __desktopRevision: document.revision };
}

export function deleteDesktopCanvasProjects(projectIds: string[]) {
    return invoke<number>("delete_desktop_canvas_projects", { projectIds });
}

export function getDesktopCanvasProjectRevision(projectId: string) {
    return invoke<string>("desktop_canvas_project_revision", { projectId });
}

export function saveCanvasExport(bytes: ArrayBuffer) {
    return invoke<{ saved: boolean; file_name?: string; bytes: number }>("save_canvas_export", bytes);
}

export function loadDesktopCanvasDeletedIds() {
    return invoke<string[]>("desktop_canvas_deleted_ids");
}

export async function restoreDesktopCanvasVersion<T>(projectId: string, sequence: number, expectedRevision: string, requestId: string): Promise<T & { __desktopRevision: string }> {
    const result = await invoke<{ project: T; revision: string }>("desktop_canvas_history_restore", { projectId, sequence, expectedRevision, requestId });
    if (!result.project || !result.revision) throw new Error("恢复结果未包含画布，请重新读取版本列表核对");
    return { ...result.project, __desktopRevision: result.revision };
}
