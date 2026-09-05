"use client";

import { useEffect, useRef, useState } from "react";
import { useParams } from "next/navigation";
import { acquireCanvasLocalImage, localCanvasImageKey } from "@/services/canvas-local-image";

import type { LocalMediaReference } from "../types";
import { acquireCanvasStoredMedia } from "@/services/canvas-media-lease";

type Lease = ReturnType<typeof acquireCanvasLocalImage>;
type LoadedImage = { identity: string; lease: Lease; src?: string; error?: string };

export function useCanvasImageSource(metadata?: { content?: string; storageKey?: string; localMedia?: LocalMediaReference }, enabled = true, observeVisibility = false, image = true) {
    const params = useParams<{ id?: string }>();
    const projectId = params?.id || "";
    const localKey = localCanvasImageKey(metadata);
    const storageKey = localKey || metadata?.storageKey || "";
    const identity = JSON.stringify([projectId, storageKey, image, metadata?.localMedia]);
    const ref = useRef<HTMLDivElement>(null);
    const leaseRef = useRef<Lease | null>(null);
    const [visible, setVisible] = useState(!observeVisibility);
    const [loaded, setLoaded] = useState<LoadedImage | null>(null);

    useEffect(() => {
        if (!observeVisibility || !storageKey || !enabled) return;
        const element = ref.current;
        if (!element) return;
        // Observe actual screen intersection, not the canvas's wider mount range.
        const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting));
        observer.observe(element);
        return () => { observer.disconnect(); setVisible(false); };
    }, [enabled, observeVisibility, storageKey]);

    useEffect(() => {
        if (!enabled || !storageKey || (localKey && !projectId) || (observeVisibility && !visible)) return;
        const lease = !image && metadata?.localMedia ? {
            url: import("@/services/desktop-runtime").then(({ resolveLocalMediaReference }) => resolveLocalMediaReference(metadata.localMedia!, projectId)).then((result) => {
                if (result.status !== "available" || !result.playbackUrl) throw new Error("本机媒体不可用，请重新定位原文件");
                return result.playbackUrl;
            }),
            release() {},
        } : localKey && image ? acquireCanvasLocalImage(projectId, localKey) : acquireCanvasStoredMedia(storageKey, metadata?.content, image, projectId);
        leaseRef.current = lease;
        let live = true;
        setLoaded(null);
        void lease.url.then(
            (src) => { if (live) setLoaded({ identity, lease, src }); },
            (error) => { if (live) setLoaded({ identity, lease, error: error instanceof Error ? error.message : String(error) }); },
        );
        return () => {
            live = false;
            if (leaseRef.current === lease) leaseRef.current = null;
            lease.release();
        };
    }, [enabled, identity, image, localKey, metadata?.content, observeVisibility, projectId, storageKey, visible]);

    const current = loaded?.identity === identity && loaded.lease === leaseRef.current ? loaded : null;
    return {
        ref,
        src: storageKey ? (enabled && (!observeVisibility || visible) ? current?.src : undefined) : metadata?.content,
        error: storageKey ? (localKey && !projectId ? "缺少当前画布绑定" : current?.error) : undefined,
    };
}
