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

export function selectLocalMedia(mode: LocalMediaReference["mode"]) {
    return invoke<LocalMediaResolution[]>("select_local_media", { mode });
}

export function resolveLocalMediaReference(reference: LocalMediaReference) {
    return invoke<LocalMediaResolution>("resolve_local_media_reference", { reference });
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

export function cancelDesktopTask(taskId: string) {
    return invoke<boolean>("cancel_desktop_task", { taskId });
}

export function listDesktopCanvasProjects<T>() {
    return invoke<T[]>("desktop_canvas_projects");
}

export function saveDesktopCanvasProject<T>(project: T) {
    return invoke<T>("save_desktop_canvas_project", { project });
}

export function deleteDesktopCanvasProjects(projectIds: string[]) {
    return invoke<number>("delete_desktop_canvas_projects", { projectIds });
}

export function getDesktopCanvasProjectRevision(projectId: string) {
    return invoke<number>("desktop_canvas_project_revision", { projectId });
}

export function saveCanvasExport(bytes: ArrayBuffer) {
    return invoke<{ saved: boolean; file_name?: string; bytes: number }>("save_canvas_export", bytes);
}

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
