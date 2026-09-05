import { createMediaReadQueue } from "./media-read-queue";
import { readCanvasMediaBlob } from "./canvas-media";
import { getImageBlob, adoptStoredImageUrl, leaseStoredImageBlob, resolveImageUrl } from "./image-storage";
import { getMediaBlob, adoptStoredMediaUrl, leaseStoredMediaBlob, resolveMediaUrl } from "./file-storage";

type Entry = { references: number; url: Promise<string>; releaseBlob?: () => void };
const entries = new Map<string, Entry>();
const readQueued = createMediaReadQueue(2);

// No idle display cache: the final lease drops its URL. Persistent blobs and
// unknown legacy consumers are managed by the underlying storage service.
export function acquireCanvasStoredMedia(storageKey: string, fallback = "", image = true, projectId = "") {
    const key = JSON.stringify([projectId, image, storageKey]);
    let entry = entries.get(key);
    if (!entry) {
        entry = { references: 0, url: Promise.resolve("") };
        const current = entry;
        entries.set(key, current);
        current.url = readQueued(async () => {
            const blob = storageKey.startsWith("local-ref:") ? await readCanvasMediaBlob(projectId, storageKey) : await (image ? getImageBlob(storageKey) : getMediaBlob(storageKey));
            if (entries.get(key) !== current) return "";
            if (blob) {
                const lease = image ? leaseStoredImageBlob(storageKey.startsWith("local-ref:") ? key : storageKey, blob) : leaseStoredMediaBlob(storageKey.startsWith("local-ref:") ? key : storageKey, blob);
                current.releaseBlob = lease.release;
                return lease.url;
            }
            const url = await (image ? resolveImageUrl(storageKey, fallback) : resolveMediaUrl(storageKey, fallback));
            const lease = image ? adoptStoredImageUrl(storageKey) : adoptStoredMediaUrl(storageKey);
            if (entries.get(key) !== current) { lease?.release(); return ""; }
            current.releaseBlob = lease?.release;
            return url;
        }, () => entries.get(key) === current).catch((error) => { if (entries.get(key) === current) entries.delete(key); throw error; });
    }
    const current = entry;
    current.references++;
    let released = false;
    return { url: current.url, release() {
        if (released) return;
        released = true;
        current.references--;
        // No consumer retains the display URL. Drop it immediately; the next
        // mount reloads exactly the original Blob from persistent storage.
        if (!current.references) {
            current.releaseBlob?.();
            if (entries.get(key) === current) entries.delete(key);
        }
    } };
}
