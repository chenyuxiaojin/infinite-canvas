import { nanoid } from "nanoid";
import { readZip } from "@/lib/zip";
import { setImageBlob } from "@/services/image-storage";
import { setMediaBlob } from "@/services/file-storage";
import type { CanvasExportFile } from "../export-types";
import type { CanvasProject } from "../stores/use-canvas-store";
import { collectStorageKeys } from "./canvas-export";
import { validateCanvasGraph } from "./canvas-graph";

export async function importCanvasArchive(file: Blob): Promise<CanvasProject[]> {
    const zip = await readZip(file);
    const manifest = zip.get("projects.json");
    if (!manifest) throw new Error("压缩包缺少画布清单");
    const data = JSON.parse(await manifest.text()) as CanvasExportFile;
    if (data.app !== "infinite-canvas" || data.version !== 3 || !Array.isArray(data.projects)) throw new Error("画布格式或版本不支持");
    // Validate every file before creating any project. Never silently accept a partial backup.
    for (const entry of data.projects) {
        if (!entry.project || !Array.isArray(entry.files)) throw new Error("画布清单不完整");
        validateCanvasGraph(entry.project);
        const keys = new Set(entry.files.map((item) => item.storageKey));
        for (const key of collectStorageKeys(entry.project)) if (!keys.has(key)) throw new Error(`备份缺少素材：${key}`);
        for (const item of entry.files) {
            const blob = zip.get(item.path);
            if (!blob || blob.size !== item.bytes) throw new Error(`素材缺失或大小不符：${item.path}`);
            if (item.sha256) {
                const hash = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", await blob.arrayBuffer())), (b) => b.toString(16).padStart(2, "0")).join("");
                if (hash !== item.sha256) throw new Error(`素材校验失败：${item.path}`);
            }
        }
    }
    const projects: CanvasProject[] = [];
    for (const entry of data.projects) {
        const mapping = new Map<string, { key: string; url: string }>();
        for (const item of entry.files) {
            const blob = zip.get(item.path)!.slice(0, item.bytes, item.mimeType);
            const image = item.mimeType.startsWith("image/");
            const key = `${image ? "image" : item.mimeType.startsWith("audio/") ? "audio" : "video"}:${nanoid()}`;
            const url = await (image ? setImageBlob(key, blob, false) : setMediaBlob(key, blob, false));
            mapping.set(item.storageKey, { key, url });
        }
        const remap = (value: unknown): unknown => {
            if (typeof value === "string") return mapping.get(value)?.key || value;
            if (Array.isArray(value)) return value.map(remap);
            if (!value || typeof value !== "object") return value;
            const object = value as Record<string, unknown>;
            const result = Object.fromEntries(Object.entries(object).map(([key, child]) => [key, remap(child)]));
            const media = typeof object.storageKey === "string" && mapping.get(object.storageKey);
            if (media) {
                result.storageKey = media.key;
                if ("content" in object) result.content = media.url;
                if ("dataUrl" in object) result.dataUrl = media.url;
                if ("url" in object) result.url = media.url;
                if (object.localMedia) {
                    result.importedMediaOrigin = JSON.stringify(object.localMedia);
                    delete result.localMedia;
                }
            }
            return result;
        };
        projects.push(remap(entry.project) as CanvasProject);
    }
    return projects;
}
