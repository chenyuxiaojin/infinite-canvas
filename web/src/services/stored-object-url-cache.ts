// Shared ownership rules for image/video caches. Legacy URL-returning callers
// have no release contract: retain those URLs until explicitly handed back.
// Leased entries retain zero idle bytes after the final consumer releases them.
export class StoredObjectUrlCache {
    private entries = new Map<string, { url: string; bytes: number; references: number; legacy: boolean }>();
    private retired = new Set<string>();
    constructor() {
        if (typeof window !== "undefined") window.addEventListener("pagehide", (event) => {
            if (event.persisted) return;
            for (const entry of this.entries.values()) URL.revokeObjectURL(entry.url);
            for (const url of this.retired) URL.revokeObjectURL(url);
            this.entries.clear(); this.retired.clear();
        });
    }
    get(key: string) {
        const entry = this.entries.get(key);
        if (entry) entry.legacy = true;
        return entry?.url;
    }
    set(key: string, url: string, bytes = 0) {
        const previous = this.entries.get(key);
        // Replacing a persistent blob must not invalidate an in-flight user.
        if (previous?.url === url) return;
        if (previous) this.retired.add(previous.url);
        this.entries.set(key, { url, bytes, references: 0, legacy: true });
    }
    delete(key: string) {
        const entry = this.entries.get(key);
        if (entry && !entry.references && !entry.legacy) URL.revokeObjectURL(entry.url);
        else if (entry) this.retired.add(entry.url);
        this.entries.delete(key);
    }
    acquire(key: string, blob: Blob) {
        let entry = this.entries.get(key);
        if (!entry) {
            entry = { url: URL.createObjectURL(blob), bytes: blob.size, references: 0, legacy: false };
            this.entries.set(key, entry);
        }
        return this.hold(key, entry);
    }
    adopt(key: string) {
        const entry = this.entries.get(key);
        return entry ? this.hold(key, entry) : undefined;
    }
    private hold(key: string, entry: { url: string; bytes: number; references: number; legacy: boolean }) {
        // A URL-returning producer hands off to an explicit scope/display owner.
        // All active consumers use independent leases before that owner closes.
        entry.legacy = false;
        entry.references++;
        const current = entry;
        let released = false;
        return { url: current.url, release: () => {
            if (released) return;
            released = true;
            current.references--;
            if (!current.references && !current.legacy) {
                URL.revokeObjectURL(current.url);
                this.retired.delete(current.url);
                if (this.entries.get(key) === current) this.entries.delete(key);
            }
        } };
    }
}
