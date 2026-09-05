import { getMediaBlob, setMediaBlob, type UploadedFile } from "@/services/file-storage";
import { fetchDesktopTaskMedia, type DesktopTaskMedia } from "@/services/desktop-runtime";

const LOCAL_TASK_STORAGE_PREFIX = "local-task";

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
    if (!existing) await setMediaBlob(storageKey, blob, false);
    const url = "";
    return {
        url,
        storageKey,
        bytes: blob.size,
        mimeType: media.mime_type,
        width: 320,
        height: 180,
        durationMs: 1_000,
        sha256: media.sha256,
    };
}

async function sha256(bytes: Uint8Array) {
    const data = new ArrayBuffer(bytes.byteLength);
    new Uint8Array(data).set(bytes);
    const digest = await crypto.subtle.digest("SHA-256", data);
    return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
