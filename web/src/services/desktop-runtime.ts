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
        status: "ok" | "error";
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

export function fetchDesktopTaskStatus(taskId: string) {
    return invoke<DesktopTaskSnapshot>("desktop_task_status", { taskId });
}

export function cancelDesktopTask(taskId: string) {
    return invoke<boolean>("cancel_desktop_task", { taskId });
}
