import { adoptStoredImageUrl } from "./image-storage";
import { adoptStoredMediaUrl } from "./file-storage";
import { acquireCanvasStoredMedia } from "./canvas-media-lease";

// Project editing operations may outlive individual mounted cards. They share
// a project owner; players/previews have independent leases and exports read
// Blob directly. Closing cannot invalidate another owner's lease.
export class CanvasMediaScope {
    private leases = new Map<string, ReturnType<typeof acquireCanvasStoredMedia>>();
    closed = false;
    constructor(readonly projectId: string) {}
    async url(storageKey: string, fallback = "", image = true) {
        if (this.closed) throw new DOMException("画布已关闭", "AbortError");
        const key = JSON.stringify([storageKey, image]);
        let lease = this.leases.get(key);
        if (!lease) {
            lease = acquireCanvasStoredMedia(storageKey, fallback, image, this.projectId);
            this.leases.set(key, lease);
        }
        try {
            const url = await lease.url;
            if (this.closed) throw new DOMException("画布已关闭", "AbortError");
            return url;
        } catch (error) {
            if (this.leases.get(key) === lease) { this.leases.delete(key); lease.release(); }
            throw error;
        }
    }
    adoptReferences(value: unknown) {
        if (this.closed || !value || typeof value !== "object") return;
        if ("storageKey" in value && typeof value.storageKey === "string") {
            for (const image of [true, false]) {
                const key = JSON.stringify([value.storageKey, image]);
                if (this.leases.has(key)) continue;
                const lease = image ? adoptStoredImageUrl(value.storageKey) : adoptStoredMediaUrl(value.storageKey);
                if (lease) this.leases.set(key, { url: Promise.resolve(lease.url), release: lease.release });
            }
        }
        for (const child of Object.values(value)) this.adoptReferences(child);
    }
    close() {
        this.closed = true;
        this.leases.forEach((lease) => lease.release());
        this.leases.clear();
    }
}
