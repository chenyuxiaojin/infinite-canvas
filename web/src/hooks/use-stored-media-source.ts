"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { acquireCanvasStoredMedia } from "@/services/canvas-media-lease";

// A display consumer owns its URL from effect setup until cleanup. Observed
// cards load original bytes only while near view; players can keep an active
// lease. Request/export code reads Blob directly instead of borrowing this URL.
export function useStoredMediaSource({ storageKey, fallback = "", image = true, projectId = "", enabled = true, observe = false, keepAlive = false }: {
    storageKey?: string; fallback?: string; image?: boolean; projectId?: string;
    enabled?: boolean; observe?: boolean; keepAlive?: boolean;
}) {
    const [element, setElement] = useState<HTMLElement | null>(null);
    const ref = useCallback((next: HTMLElement | null) => setElement(next), []);
    const [visible, setVisible] = useState(!observe);
    const identity = JSON.stringify([storageKey, fallback, image, projectId]);
    const [loaded, setLoaded] = useState<{ identity: string; generation: number; src?: string; error?: string } | null>(null);
    const active = enabled && (!observe || visible || keepAlive);
    const generation = useRef(0);
    useEffect(() => {
        if (!observe || !element) return;
        const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), { rootMargin: "160px" });
        observer.observe(element);
        return () => observer.disconnect();
    }, [element, observe]);
    useEffect(() => {
        const current = ++generation.current;
        if (!active || !storageKey) return;
        const lease = acquireCanvasStoredMedia(storageKey, fallback, image, projectId);
        void lease.url.then(
            (src) => { if (generation.current === current) setLoaded({ identity, generation: current, src }); },
            (error) => { if (generation.current === current) setLoaded({ identity, generation: current, error: error instanceof Error ? error.message : String(error) }); },
        );
        return () => { generation.current++; lease.release(); };
    }, [active, identity, storageKey, fallback, image, projectId]);
    return { ref, src: !active ? "" : !storageKey ? fallback : loaded?.identity === identity && loaded.generation === generation.current ? loaded.src || "" : "", error: active && loaded?.identity === identity && loaded.generation === generation.current ? loaded.error : undefined };
}
