import type { Asset } from "@/stores/use-asset-store";

// Remove only an ephemeral copy of the same persisted primary media. Custom
// covers and assets without a stable key stay intact.
export function stableAssetMedia<T extends Asset>(asset: T): T {
    if (asset.kind === "text" || !asset.data.storageKey) return asset;
    const url = asset.kind === "image" ? asset.data.dataUrl : asset.data.url;
    const stableUrl = url?.startsWith("blob:") ? "" : url;
    const coverUrl = (asset.coverUrl === url || asset.kind === "image") && asset.coverUrl?.startsWith("blob:") ? "" : asset.coverUrl;
    return { ...asset, coverUrl, data: { ...asset.data, ...(asset.kind === "image" ? { dataUrl: stableUrl } : { url: stableUrl }) } } as T;
}

export function assetMediaReference(asset: Asset | null, cover = false) {
    if (!asset) return { enabled: false };
    const fallback = asset.kind === "text" ? asset.coverUrl : asset.kind === "image" ? asset.data.dataUrl : asset.data.url;
    const separateCover = cover && asset.coverUrl && asset.coverUrl !== fallback;
    return {
        enabled: asset.kind !== "text" || Boolean(asset.coverUrl),
        storageKey: separateCover || asset.kind === "text" ? undefined : asset.data.storageKey,
        fallback: separateCover ? asset.coverUrl : fallback,
        image: Boolean(separateCover) || asset.kind === "image" || asset.kind === "text",
    };
}
