import type { UploadedFile } from "@/services/file-storage";
import { fetchDesktopTaskMediaReference } from "@/services/desktop-runtime";
import type { LocalMediaReference } from "../types";
import type { CanvasNodeMetadata } from "../types";

const LOCAL_TASK_STORAGE_PREFIX = "local-task";

export function desktopTaskIdFromStorageKey(storageKey?: string) {
    const prefix = `${LOCAL_TASK_STORAGE_PREFIX}:`;
    if (!storageKey?.startsWith(prefix)) return null;
    const taskId = storageKey.slice(prefix.length);
    return taskId && !taskId.includes(":") ? taskId : null;
}

export function materializeDesktopTaskMetadata(metadata: CanvasNodeMetadata, content: string): CanvasNodeMetadata {
    return { ...metadata, content };
}

export async function resolveDesktopTaskMedia(taskId: string): Promise<UploadedFile & { sha256: string; localMedia: LocalMediaReference }> {
    const resolution = await fetchDesktopTaskMediaReference(taskId);
    const media = resolution.reference;
    if (resolution.status !== "available" || !resolution.playbackUrl || media.mimeType !== "video/mp4") throw new Error("本地任务媒体不可用或回执不匹配");
    if (!media.width || !media.height || !media.durationMs) throw new Error("本地任务媒体探测信息不完整");
    return {
        url: resolution.playbackUrl,
        storageKey: media.storageKey,
        bytes: media.bytes,
        mimeType: media.mimeType,
        width: media.width,
        height: media.height,
        durationMs: media.durationMs,
        sha256: media.sha256,
        localMedia: media,
    };
}
