import { getMediaBlob, resolveMediaUrl, setMediaBlob, type UploadedFile } from "@/services/file-storage";
import { fetchDesktopTaskMedia, type DesktopTaskMedia } from "@/services/desktop-runtime";
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

export async function persistDesktopTaskMedia(taskId: string): Promise<UploadedFile & { sha256: string }> {
    const media = await fetchDesktopTaskMedia(taskId);
    if (media.task_id !== taskId || media.mime_type !== "video/mp4") throw new Error("本地任务媒体回执不匹配");
    const bytes = Uint8Array.from(media.bytes);
    const actualSha256 = await sha256(bytes);
    if (actualSha256 !== media.sha256) throw new Error("本地任务媒体哈希校验失败");

    const storageKey = `${LOCAL_TASK_STORAGE_PREFIX}:${taskId}`;
    const existing = await getMediaBlob(storageKey);
    if (existing) {
        const existingSha256 = await sha256(new Uint8Array(await existing.arrayBuffer()));
        if (existingSha256 !== media.sha256) throw new Error("本地任务输出冲突：现有媒体与任务回执不同");
    }

    const blob = existing || new Blob([bytes], { type: media.mime_type });
    const url = existing ? await resolveMediaUrl(storageKey) : await setMediaBlob(storageKey, blob);
    if (!url) throw new Error("本地任务媒体无法载入画布");
    const videoStream = media.probe.streams.find((stream) => stream.codec_type === "video");
    const width = videoStream?.width;
    const height = videoStream?.height;
    if (!width || !height || !media.probe.duration_ms) throw new Error("本地任务媒体探测信息不完整");
    return {
        url,
        storageKey,
        bytes: blob.size,
        mimeType: media.mime_type,
        width,
        height,
        durationMs: media.probe.duration_ms,
        sha256: media.sha256,
    };
}

async function sha256(bytes: Uint8Array) {
    const data = new ArrayBuffer(bytes.byteLength);
    new Uint8Array(data).set(bytes);
    const digest = await crypto.subtle.digest("SHA-256", data);
    return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
