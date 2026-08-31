import { saveAs } from "file-saver";

import { createZip } from "@/lib/zip";
import { getMediaBlob } from "@/services/file-storage";
import { getImageBlob } from "@/services/image-storage";
import { fetchDesktopTaskMediaReference, isDesktopRuntime, resolveLocalMediaReference, saveCanvasExport, saveCanvasExportWithLocalMedia } from "@/services/desktop-runtime";
import type { CanvasExportAsset, CanvasExportFile } from "../export-types";
import type { CanvasProject } from "../stores/use-canvas-store";
import type { LocalMediaReference } from "../types";
import { desktopTaskIdFromStorageKey } from "./canvas-local-task";

export type CanvasExportMediaMode = "embedded" | "references";

export async function exportCanvasProjects(projects: CanvasProject[], fileName = "无限画布", mediaMode: CanvasExportMediaMode = "embedded") {
    const zipFiles: { name: string; data: BlobPart }[] = [];
    const nativeLocalFiles: Array<{ path: string; reference: LocalMediaReference }> = [];
    const exportedProjects = await Promise.all(
        projects.map(async (sourceProject) => {
            const project = structuredClone(sourceProject);
            const entries = collectStorageEntries(project);
            const files: CanvasExportAsset[] = [];
            for (const [storageKey, storedReference] of entries) {
                const taskId = desktopTaskIdFromStorageKey(storageKey);
                const reference = storedReference || (taskId && isDesktopRuntime() ? (await fetchDesktopTaskMediaReference(taskId)).reference : undefined);
                if (reference) {
                    attachCurrentNodeReference(project, storageKey, reference);
                    const path = `projects/${project.id}/files/${safeFileName(storageKey)}.${fileExtension(reference.mimeType, storageKey)}`;
                    files.push({ storageKey, path, mimeType: reference.mimeType, bytes: reference.bytes, embedded: mediaMode === "embedded", reference });
                    if (mediaMode === "embedded") {
                        if (isDesktopRuntime()) nativeLocalFiles.push({ path, reference });
                        else {
                            const resolution = await resolveLocalMediaReference(reference);
                            if (resolution.status !== "available" || !resolution.playbackUrl) throw new Error(`本机素材不可用：${reference.fileName}`);
                            zipFiles.push({ name: path, data: await (await fetch(resolution.playbackUrl)).blob() });
                        }
                    }
                    continue;
                }
                const blob = storageKey.startsWith("image:") ? await getImageBlob(storageKey) : await getMediaBlob(storageKey);
                const path = `projects/${project.id}/files/${safeFileName(storageKey)}.${fileExtension(blob?.type || "application/octet-stream", storageKey)}`;
                files.push({ storageKey, path, mimeType: blob?.type || "application/octet-stream", bytes: blob?.size || 0, embedded: mediaMode === "embedded" && Boolean(blob) });
                if (mediaMode === "embedded" && blob) zipFiles.push({ name: path, data: blob });
            }
            return { project, files };
        }),
    );

    const data: CanvasExportFile = { app: "infinite-canvas", version: 5, exportedAt: new Date().toISOString(), mediaMode, projects: exportedProjects };
    const zip = await createZip([{ name: "projects.json", data: JSON.stringify(data, null, 2) }, ...zipFiles]);
    if (isDesktopRuntime()) {
        if (nativeLocalFiles.length) await saveCanvasExportWithLocalMedia(await zip.arrayBuffer(), nativeLocalFiles);
        else await saveCanvasExport(await zip.arrayBuffer());
        return;
    }
    saveAs(zip, `${safeFileName(fileName)}.zip`);
}

function collectStorageEntries(value: unknown, entries = new Map<string, LocalMediaReference | undefined>()) {
    if (!value || typeof value !== "object") return entries;
    if ("storageKey" in value && typeof value.storageKey === "string" && value.storageKey.includes(":")) {
        const reference = "localMedia" in value && isLocalMediaReference(value.localMedia) ? value.localMedia : undefined;
        entries.set(value.storageKey, entries.get(value.storageKey) || reference);
    }
    Object.values(value).forEach((item) => (Array.isArray(item) ? item.forEach((child) => collectStorageEntries(child, entries)) : collectStorageEntries(item, entries)));
    return entries;
}

function attachCurrentNodeReference(project: CanvasProject, storageKey: string, reference: LocalMediaReference) {
    project.nodes = project.nodes.map((node) => {
        if (node.metadata?.storageKey !== storageKey) return node;
        return {
            ...node,
            metadata: {
                ...node.metadata,
                content: reference.storageKey,
                storageKey: reference.storageKey,
                localMedia: reference,
                localMediaRuntime: undefined,
            },
        };
    });
}

function isLocalMediaReference(value: unknown): value is LocalMediaReference {
    if (!value || typeof value !== "object") return false;
    const reference = value as Partial<LocalMediaReference>;
    return typeof reference.assetId === "string" && typeof reference.storageKey === "string" && typeof reference.rootId === "string" && typeof reference.relativePath === "string" && typeof reference.sha256 === "string";
}

function safeFileName(value: string) {
    return value.replace(/[\\/:*?"<>|]/g, "_");
}

function fileExtension(mimeType: string, storageKey: string) {
    if (mimeType.includes("png")) return "png";
    if (mimeType.includes("jpeg")) return "jpg";
    if (mimeType.includes("webp")) return "webp";
    if (mimeType.includes("gif")) return "gif";
    if (mimeType.includes("mp4")) return mimeType.startsWith("audio/") ? "m4a" : "mp4";
    if (mimeType.includes("quicktime")) return "mov";
    if (mimeType.includes("webm")) return "webm";
    if (mimeType.includes("mpeg")) return "mp3";
    if (mimeType.includes("wav")) return "wav";
    return storageKey.startsWith("image:") ? "png" : "bin";
}
