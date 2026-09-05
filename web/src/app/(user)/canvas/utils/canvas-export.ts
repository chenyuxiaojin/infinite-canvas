import { saveAs } from "file-saver";

import { createZip } from "@/lib/zip";
import { readCanvasMediaBlob } from "@/services/canvas-media";
import { isDesktopRuntime, saveCanvasExport } from "@/services/desktop-runtime";
import type { CanvasExportAsset, CanvasExportFile } from "../export-types";
import type { CanvasProject } from "../stores/use-canvas-store";

export async function exportCanvasProjects(projects: CanvasProject[], fileName = "小陈的画布") {
    const zipFiles: { name: string; data: BlobPart }[] = [];
    const exportedProjects = [];
    for (const project of projects) {
        const files: CanvasExportAsset[] = [];
        // Read sequentially: real 4K images must not create dozens of concurrent IPC reads.
        for (const storageKey of collectStorageKeys(project)) {
            let blob: Blob;
            try { blob = await readCanvasMediaBlob(project.id, storageKey, findMimeType(project, storageKey)); }
            catch (error) { throw new Error(`画布「${project.title}」未能完整导出：${error instanceof Error ? error.message : String(error)}`); }
            const path = `projects/${project.id}/files/${safeFileName(storageKey)}.${fileExtension(blob.type, storageKey)}`;
            const sha256 = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", await blob.arrayBuffer())), (b) => b.toString(16).padStart(2, "0")).join("");
            files.push({ storageKey, path, mimeType: blob.type || "application/octet-stream", bytes: blob.size, sha256 });
            zipFiles.push({ name: path, data: blob });
        }
        exportedProjects.push({ project, files });
    }

    const data: CanvasExportFile = { app: "infinite-canvas", version: 3, exportedAt: new Date().toISOString(), projects: exportedProjects };
    const zip = await createZip([{ name: "projects.json", data: JSON.stringify(data, null, 2) }, ...zipFiles]);
    if (isDesktopRuntime()) {
        return saveCanvasExport(await zip.arrayBuffer());
    }
    saveAs(zip, `${safeFileName(fileName)}.zip`);
}

export function collectStorageKeys(value: unknown, keys = new Set<string>()) {
    if (!value || typeof value !== "object") return [...keys];
    if ("storageKey" in value && typeof value.storageKey === "string" && value.storageKey.includes(":")) keys.add(value.storageKey);
    Object.values(value).forEach((item) => (Array.isArray(item) ? item.forEach((child) => collectStorageKeys(child, keys)) : collectStorageKeys(item, keys)));
    return [...keys];
}

function safeFileName(value: string) {
    return value.replace(/[\\/:*?"<>|]/g, "_");
}

function fileExtension(mimeType: string, storageKey: string) {
    if (mimeType.includes("png")) return "png";
    if (mimeType.includes("jpeg")) return "jpg";
    if (mimeType.includes("webp")) return "webp";
    if (mimeType.includes("gif")) return "gif";
    if (mimeType.includes("mp4")) return "mp4";
    if (mimeType.includes("webm")) return "webm";
    return storageKey.startsWith("image:") ? "png" : "bin";
}

function findMimeType(value: unknown, key: string): string | undefined {
    if (!value || typeof value !== "object") return;
    if ("storageKey" in value && value.storageKey === key && "mimeType" in value && typeof value.mimeType === "string") return value.mimeType;
    for (const child of Object.values(value)) { const mime = findMimeType(child, key); if (mime) return mime; }
}
