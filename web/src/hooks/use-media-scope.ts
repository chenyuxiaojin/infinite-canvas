"use client";
import { useCallback, useEffect, useRef } from "react";
import { CanvasMediaScope } from "@/services/canvas-media-scope";
import { uploadImage as persistImage } from "@/services/image-storage";
import { uploadMediaFile as persistMedia } from "@/services/file-storage";

export function useMediaScope(ownerId: string) {
    const scopeRef = useRef(new CanvasMediaScope(ownerId));
    useEffect(() => {
        if (scopeRef.current.closed) scopeRef.current = new CanvasMediaScope(ownerId);
        return () => scopeRef.current.close();
    }, [ownerId]);
    const uploadImage = useCallback(async (...args: Parameters<typeof persistImage>) => {
        const scope = scopeRef.current;
        const stored = await persistImage(args[0], { ...args[1], retainDisplayUrl: false });
        return { ...stored, url: await scope.url(stored.storageKey, stored.url, true) };
    }, []);
    const uploadMediaFile = useCallback(async (...args: Parameters<typeof persistMedia>) => {
        const scope = scopeRef.current;
        const stored = await persistMedia(args[0], args[1], false);
        return { ...stored, url: await scope.url(stored.storageKey, stored.url, false) };
    }, []);
    const resolveImageUrl = useCallback((key?: string, fallback = "") => key ? scopeRef.current.url(key, fallback, true) : Promise.resolve(fallback), []);
    const resolveMediaUrl = useCallback((key?: string, fallback = "") => key ? scopeRef.current.url(key, fallback, false) : Promise.resolve(fallback), []);
    return { scopeRef, uploadImage, uploadMediaFile, resolveImageUrl, resolveMediaUrl };
}
