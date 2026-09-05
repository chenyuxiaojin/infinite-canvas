import { invoke, isTauri } from "@tauri-apps/api/core";

type ImageReference = { content?: string; storageKey?: string };

export function localCanvasImageKey(metadata?: ImageReference) {
    if (metadata?.storageKey?.startsWith("local-ref:")) return metadata.storageKey;
    return metadata?.content?.startsWith("local-ref:") ? metadata.content : "";
}

export function hasCanvasImageSource(metadata?: ImageReference) {
    return Boolean(metadata?.content || metadata?.storageKey);
}

const MAX_IMAGE_BYTES = 64 * 1024 * 1024;
const MAX_READS = 2;

export function imageMimeType(buffer: ArrayBuffer) {
    const bytes = new Uint8Array(buffer);
    const matches = (signature: number[], offset = 0) => signature.every((byte, index) => bytes[offset + index] === byte);
    if (matches([137, 80, 78, 71, 13, 10, 26, 10])) return "image/png";
    if (matches([255, 216, 255])) return "image/jpeg";
    if (matches([71, 73, 70, 56, 55, 97]) || matches([71, 73, 70, 56, 57, 97])) return "image/gif";
    if (matches([82, 73, 70, 70]) && matches([87, 69, 66, 80], 8)) return "image/webp";
    throw new Error("本地素材不是支持的 PNG、JPEG、GIF 或 WebP 图片");
}

type ImageEntry = {
    key: string;
    projectId: string;
    storageKey: string;
    references: number;
    started: boolean;
    settled: boolean;
    url?: string;
    promise: Promise<string>;
    resolve: (url: string) => void;
    reject: (error: unknown) => void;
};

// Display-only leases: never put these URLs in a canvas node, IndexedDB or the
// existing permanent image cache. The node and fullscreen preview share a lease.
const images = new Map<string, ImageEntry>();
const queue: ImageEntry[] = [];
let activeReads = 0;

function forget(entry: ImageEntry) {
    if (images.get(entry.key) === entry) images.delete(entry.key);
}

function drainQueue() {
    while (activeReads < MAX_READS && queue.length) {
        const entry = queue.shift()!;
        entry.started = true;
        activeReads++;
        void (async () => {
            try {
                if (!isTauri()) throw new Error("本地片子图片需要在桌面应用中打开");
                const bytes = await invoke<ArrayBuffer>("read_canvas_local_image", { projectId: entry.projectId, storageKey: entry.storageKey });
                if (!(bytes instanceof ArrayBuffer) || !bytes.byteLength || bytes.byteLength > MAX_IMAGE_BYTES) throw new Error("本地图片大小无效或超过 64 MiB");
                if (!entry.references) {
                    entry.resolve("");
                    return;
                }
                entry.url = URL.createObjectURL(new Blob([bytes], { type: imageMimeType(bytes) }));
                entry.resolve(entry.url);
            } catch (error) {
                entry.reject(error);
            } finally {
                entry.settled = true;
                if (!entry.references) forget(entry);
                activeReads--;
                drainQueue();
            }
        })();
    }
}

export function acquireCanvasLocalImage(projectId: string, storageKey: string) {
    if (!projectId || !storageKey.startsWith("local-ref:")) throw new Error("缺少当前画布或已登记的图片引用");
    const key = JSON.stringify([projectId, storageKey]);
    let entry = images.get(key);
    if (!entry) {
        let resolve!: ImageEntry["resolve"];
        let reject!: ImageEntry["reject"];
        const promise = new Promise<string>((yes, no) => { resolve = yes; reject = no; });
        entry = { key, projectId, storageKey, references: 0, started: false, settled: false, promise, resolve, reject };
        images.set(key, entry);
        queue.push(entry);
        queueMicrotask(drainQueue);
    }
    entry.references++;
    const current = entry;
    let released = false;
    return {
        url: current.promise,
        release() {
            if (released) return;
            released = true;
            if (--current.references) return;
            if (current.url) URL.revokeObjectURL(current.url);
            if (!current.started) {
                queue.splice(queue.indexOf(current), 1);
                current.resolve("");
                forget(current);
            } else if (current.settled) {
                forget(current);
            }
            // An IPC read already in flight cannot be cancelled. Its completion
            // drops the bytes unless a new visible consumer reacquires the lease.
        },
    };
}
