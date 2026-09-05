import { invoke, isTauri } from "@tauri-apps/api/core";
import type { LocalMediaReference } from "@/app/(user)/canvas/types";

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
                width?: number;
                height?: number;
                sample_rate?: number;
                channels?: number;
            }>;
        };
    };
    error?: {
        code: string;
        message: string;
        retryable: boolean;
        side_effects_may_exist: boolean;
    };
    local_media?: LocalMediaResolution;
};

export type LocalMediaResolution = {
    reference: LocalMediaReference;
    status: "available" | "missing";
    playbackUrl?: string;
    reason?: "missing" | "digest_mismatch" | "denied" | "unavailable";
};

export type LocalMediaImportOutcome = {
    resolution: LocalMediaResolution;
    /** referenced = 原地引用；moved = 临时文件已移进素材目录；copied = 复制了一份 */
    action: "referenced" | "moved" | "copied";
    destination: "in_place" | "project_directory" | "managed_root";
    temporarySource: boolean;
};

export type LocalMediaRequestEvidence = {
    assetId: string;
    method: "GET" | "HEAD";
    requestedRange?: string;
    status: number;
    responseBytes: number;
    recordedAtMs: number;
};

export type DesktopCanvasImportResult<T> = {
    sourceVersion: number;
    projects: T[];
    importedMedia: number;
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

export function fetchDesktopTaskMediaReference(taskId: string) {
    return invoke<LocalMediaResolution>("desktop_task_media_reference", { taskId });
}

export function selectLocalMedia(mode: LocalMediaReference["mode"], projectId?: string) {
    return invoke<LocalMediaImportOutcome[]>("select_local_media", { mode, projectId });
}

/** 原生拖放拿到的是真实路径：交给桌面端按同一套收编策略登记（不经过浏览器上传） */
export function importLocalMediaPaths(projectId: string | undefined, paths: string[], mode: LocalMediaReference["mode"]) {
    return invoke<LocalMediaImportOutcome[]>("import_local_media_paths", { projectId, paths, mode });
}

export function getProjectMediaDirectory(projectId: string) {
    return invoke<string | null>("project_media_directory", { projectId });
}

export function selectProjectMediaDirectory(projectId: string) {
    return invoke<string | null>("select_project_media_directory", { projectId });
}

export function resolveLocalMediaReference(reference: LocalMediaReference, projectId?: string) {
    return invoke<LocalMediaResolution>("resolve_local_media_reference", { reference, projectId });
}

export function relinkLocalMediaReference(reference: LocalMediaReference) {
    return invoke<LocalMediaResolution | null>("relink_local_media_reference", { reference });
}

export function getLocalMediaRequestEvidence() {
    return invoke<LocalMediaRequestEvidence[]>("local_media_request_evidence");
}

export function importDesktopCanvasArchive<T>() {
    return invoke<DesktopCanvasImportResult<T> | null>("import_canvas_archive");
}

export function approvePaidGeneration(projectId: string, taskId: string) {
    return invoke<{ approved: boolean; task_id: string }>("approve_paid_generation", { projectId, taskId });
}

export function rejectPaidGeneration(projectId: string, taskId: string, reason?: string) {
    return invoke<{ rejected: boolean; task_id: string }>("reject_paid_generation", { projectId, taskId, reason });
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

export function getDesktopCanvasProjectUpdatedAt(projectId: string) { return invoke<string>("desktop_canvas_project_updated_at", { projectId }); }

export function saveCanvasExportWithLocalMedia(baseZip: ArrayBuffer, localFiles: Array<{ path: string; reference: LocalMediaReference }>) {
    const manifest = new TextEncoder().encode(JSON.stringify({ version: 1, localFiles }));
    const base = new Uint8Array(baseZip);
    const payload = new Uint8Array(8 + manifest.byteLength + base.byteLength);
    payload.set([0x49, 0x43, 0x58, 0x35], 0);
    new DataView(payload.buffer).setUint32(4, manifest.byteLength, false);
    payload.set(manifest, 8);
    payload.set(base, 8 + manifest.byteLength);
    return saveCanvasExport(payload.buffer);
}
