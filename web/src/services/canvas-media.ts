import { invoke, isTauri } from "@tauri-apps/api/core";
import { getMediaBlob } from "@/services/file-storage";
import { getImageBlob, imageToDataUrl } from "@/services/image-storage";
import type { CanvasAssistantReference } from "@/app/(user)/canvas/types";

export async function readCanvasMediaBlob(projectId: string, storageKey: string, mimeType?: string) {
    if (storageKey.startsWith("local-ref:")) {
        if (!isTauri() || !projectId) throw new Error("本地登记素材需要在对应的桌面画布中读取");
        const bytes = await invoke<ArrayBuffer>("read_canvas_local_media", { projectId, storageKey });
        if (!(bytes instanceof ArrayBuffer) || !bytes.byteLength) throw new Error("本地素材读取结果为空");
        return new Blob([bytes], { type: mimeType || "application/octet-stream" });
    }
    const blob = await getImageBlob(storageKey) || await getMediaBlob(storageKey);
    if (!blob) throw new Error(`找不到素材：${storageKey}`);
    return blob;
}

function blobDataUrl(blob: Blob) {
    return new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(new Error("无法读取图片内容"));
        reader.readAsDataURL(blob);
    });
}

// Resolve only at the send boundary; transient bytes must never enter saved nodes.
export async function resolveCanvasModelReferences(projectId: string, references: CanvasAssistantReference[]) {
    const resolved: CanvasAssistantReference[] = [];
    for (const reference of references) {
        const key = reference.storageKey || (reference.dataUrl?.startsWith("local-ref:") ? reference.dataUrl : "");
        if (!reference.dataUrl && !(key && reference.mimeType?.startsWith("image/"))) {
            resolved.push(reference);
            continue;
        }
        try {
            const dataUrl = key.startsWith("local-ref:")
                ? await blobDataUrl(await readCanvasMediaBlob(projectId, key, reference.mimeType || "image/png"))
                : await imageToDataUrl(reference);
            if (!/^data:image\//.test(dataUrl)) throw new Error("图片未解析成可发送的内容");
            resolved.push({ ...reference, dataUrl });
        } catch (error) {
            throw new Error(`图片「${reference.title || reference.id}」读取失败：${error instanceof Error ? error.message : String(error)}`);
        }
    }
    return resolved;
}
