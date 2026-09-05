// Network metadata only: choosing a remote reference must not allocate a local
// Blob URL just to discover that it is not a public URL.
export async function mediaPublicUrl(storageKey?: string, fallback = "") {
    if (/^https?:\/\//i.test(fallback)) return fallback;
    if (!storageKey?.startsWith("server:") || storageKey.startsWith("server:webdav:")) return "";
    const { getStorageObjectInfo } = await import("@/services/api/storage");
    const info = await getStorageObjectInfo(storageKey.slice("server:".length)).catch(() => null);
    return info?.publicUrl || "";
}
