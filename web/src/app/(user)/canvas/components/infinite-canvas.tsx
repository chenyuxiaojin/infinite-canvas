"use client";

import React, { useEffect, useLayoutEffect, useRef, useState } from "react";

import { canvasThemes, type CanvasBackgroundMode } from "@/lib/canvas-theme";
import { useThemeStore } from "@/stores/use-theme-store";
import type { ViewportTransform } from "../types";

type InfiniteCanvasProps = {
    containerRef: React.RefObject<HTMLDivElement | null>;
    viewport: ViewportTransform;
    tool: "select" | "pan";
    backgroundMode?: CanvasBackgroundMode;
    onViewportChange: (viewport: ViewportTransform, immediate?: boolean) => void;
    onCanvasMouseDown?: (event: React.PointerEvent<HTMLDivElement>) => void;
    onCanvasDeselect?: () => void;
    onCanvasDoubleClick?: (event: React.MouseEvent<HTMLDivElement>) => void;
    onContextMenu?: (event: React.MouseEvent) => void;
    onDrop?: (event: React.DragEvent<HTMLDivElement>) => void;
    children: React.ReactNode;
};

export function applyCanvasViewport(container: HTMLElement | null, viewport: ViewportTransform) {
    if (!container) return;
    const world = container.querySelector<HTMLElement>("[data-canvas-world]");
    if (world) {
        world.style.transform = `translate(${viewport.x}px, ${viewport.y}px) scale(${viewport.k})`;
        world.style.setProperty("--canvas-inverse-scale", String(1 / viewport.k));
    }
    const grid = container.querySelector<HTMLElement>("[data-canvas-grid]");
    if (!grid) return;
    const mode = grid.dataset.mode || "lines";
    if (mode === "blank") {
        grid.style.backgroundImage = "none";
        return;
    }
    const gridSize = 48 * viewport.k;
    const dotSize = viewport.k < 0.12 ? 0.8 : 1.15;
    const line = grid.dataset.line || "transparent";
    const dot = grid.dataset.dot || "transparent";
    grid.style.backgroundSize = `${gridSize}px ${gridSize}px`;
    grid.style.backgroundPosition = `${viewport.x % gridSize}px ${viewport.y % gridSize}px`;
    grid.style.backgroundImage =
        mode === "dots"
            ? `radial-gradient(circle, ${dot} ${dotSize}px, transparent ${dotSize + 0.2}px)`
            : `linear-gradient(${line} 1px, transparent 1px), linear-gradient(90deg, ${line} 1px, transparent 1px)`;
}

export function InfiniteCanvas({ containerRef, viewport, tool, backgroundMode = "lines", onViewportChange, onCanvasMouseDown, onCanvasDeselect, onCanvasDoubleClick, onContextMenu, onDrop, children }: InfiniteCanvasProps) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const panState = useRef({
        isPanning: false,
        startX: 0,
        startY: 0,
        initialX: 0,
        initialY: 0,
        hasMoved: false,
        startedOnBackground: false,
    });
    const liveViewportRef = useRef(viewport);
    const interactingRef = useRef(false);
    const frameRef = useRef<number | null>(null);
    const wheelCommitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const nextViewportRef = useRef<ViewportTransform | null>(null);
    const onViewportChangeRef = useRef(onViewportChange);
    const onCanvasDeselectRef = useRef(onCanvasDeselect);
    const [isSpacePressed, setIsSpacePressed] = useState(false);
    const [isPanning, setIsPanning] = useState(false);

    onViewportChangeRef.current = onViewportChange;
    onCanvasDeselectRef.current = onCanvasDeselect;

    const publishViewportRef = useRef((next: ViewportTransform, immediate = false) => {
        liveViewportRef.current = next;
        applyCanvasViewport(containerRef.current, next);
        if (frameRef.current) cancelAnimationFrame(frameRef.current);
        if (immediate) {
            frameRef.current = null;
            nextViewportRef.current = null;
            onViewportChangeRef.current(next, true);
            return;
        }
        nextViewportRef.current = next;
        frameRef.current = requestAnimationFrame(() => {
            frameRef.current = null;
            if (nextViewportRef.current) onViewportChangeRef.current(nextViewportRef.current);
        });
    });

    useLayoutEffect(() => {
        if (interactingRef.current) return;
        liveViewportRef.current = viewport;
        applyCanvasViewport(containerRef.current, viewport);
    }, [containerRef, viewport, backgroundMode, theme]);

    useEffect(
        () => () => {
            if (frameRef.current) cancelAnimationFrame(frameRef.current);
            if (wheelCommitTimerRef.current) clearTimeout(wheelCommitTimerRef.current);
        },
        [],
    );

    useEffect(() => {
        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.code !== "Space") return;
            const target = event.target instanceof Element ? event.target : null;
            if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLSelectElement || target?.closest("[contenteditable='true']")) return;
            event.preventDefault();
            setIsSpacePressed(true);
        };

        const handleKeyUp = (event: KeyboardEvent) => {
            if (event.code === "Space") {
                const target = event.target instanceof Element ? event.target : null;
                if (!(event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLSelectElement || target?.closest("[contenteditable='true']"))) event.preventDefault();
                setIsSpacePressed(false);
            }
        };

        const handleBlur = () => {
            setIsSpacePressed(false);
            if (panState.current.isPanning) {
                panState.current.isPanning = false;
                interactingRef.current = false;
                publishViewportRef.current(liveViewportRef.current, true);
            }
            setIsPanning(false);
            document.body.style.cursor = "";
        };

        window.addEventListener("keydown", handleKeyDown);
        window.addEventListener("keyup", handleKeyUp);
        window.addEventListener("blur", handleBlur);
        return () => {
            window.removeEventListener("keydown", handleKeyDown);
            window.removeEventListener("keyup", handleKeyUp);
            window.removeEventListener("blur", handleBlur);
        };
    }, []);

    const handleWheel = (event: React.WheelEvent<HTMLDivElement>) => {
        const target = event.target instanceof Element ? event.target : null;
        if (target?.closest("[data-canvas-no-zoom],.ant-modal,.ant-popover,.ant-dropdown,.ant-select-dropdown,.ant-picker-dropdown")) return;

        const current = liveViewportRef.current;
        const delta = -event.deltaY;
        const factor = Math.pow(1.1, delta / 100);
        const newScale = Math.min(Math.max(current.k * factor, 0.05), 5);
        const rect = containerRef.current?.getBoundingClientRect();
        if (!rect) return;

        const mouseX = event.clientX - rect.left;
        const mouseY = event.clientY - rect.top;
        const worldX = (mouseX - current.x) / current.k;
        const worldY = (mouseY - current.y) / current.k;

        interactingRef.current = true;
        publishViewportRef.current({
            x: mouseX - worldX * newScale,
            y: mouseY - worldY * newScale,
            k: newScale,
        });
        if (wheelCommitTimerRef.current) clearTimeout(wheelCommitTimerRef.current);
        wheelCommitTimerRef.current = setTimeout(() => {
            wheelCommitTimerRef.current = null;
            if (panState.current.isPanning) return;
            interactingRef.current = false;
            publishViewportRef.current(liveViewportRef.current, true);
        }, 120);
    };

    const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
        const target = event.target instanceof Element ? event.target : null;
        if (target?.closest("[data-canvas-no-zoom]")) return;
        if (target?.closest("[data-connection-create-menu]")) return;
        const isBackgroundClick = !target?.closest("[data-node-id],[data-connection-id]");
        if (event.button === 0 && isBackgroundClick && document.activeElement instanceof HTMLElement && (document.activeElement.isContentEditable || document.activeElement instanceof HTMLMediaElement)) document.activeElement.blur();
        const temporaryTool = isSpacePressed;
        const activeTool = temporaryTool ? (tool === "select" ? "pan" : "select") : tool;
        const shouldPan = event.button === 1 || (event.button === 0 && activeTool === "pan");
        const current = liveViewportRef.current;

        if (shouldPan) {
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            interactingRef.current = true;
            panState.current = {
                isPanning: true,
                startX: event.clientX,
                startY: event.clientY,
                initialX: current.x,
                initialY: current.y,
                hasMoved: false,
                startedOnBackground: isBackgroundClick,
            };
            setIsPanning(true);
            document.body.style.cursor = "grabbing";
            return;
        }

        if (event.button === 0 && isBackgroundClick) {
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            onCanvasMouseDown?.(event);
        }
    };

    const handleDoubleClick = (event: React.MouseEvent<HTMLDivElement>) => {
        const target = event.target instanceof Element ? event.target : null;
        if (target?.closest("[data-canvas-no-zoom],[data-node-id],[data-connection-id],[data-connection-create-menu]")) return;
        onCanvasDoubleClick?.(event);
    };

    useEffect(() => {
        const handlePointerMove = (event: PointerEvent) => {
            if (!panState.current.isPanning) return;
            if (event.buttons === 0) {
                panState.current.isPanning = false;
                interactingRef.current = false;
                setIsPanning(false);
                document.body.style.cursor = "";
                publishViewportRef.current(liveViewportRef.current, true);
                return;
            }
            const dx = event.clientX - panState.current.startX;
            const dy = event.clientY - panState.current.startY;
            if (Math.abs(dx) > 3 || Math.abs(dy) > 3) {
                panState.current.hasMoved = true;
            }

            publishViewportRef.current({
                x: panState.current.initialX + dx,
                y: panState.current.initialY + dy,
                k: liveViewportRef.current.k,
            });
        };

        const handlePointerUp = () => {
            if (!panState.current.isPanning) return;

            if (!panState.current.hasMoved && panState.current.startedOnBackground) {
                onCanvasDeselectRef.current?.();
            }
            panState.current.isPanning = false;
            interactingRef.current = false;
            setIsPanning(false);
            document.body.style.cursor = "";
            publishViewportRef.current(liveViewportRef.current, true);
        };

        window.addEventListener("pointermove", handlePointerMove);
        window.addEventListener("pointerup", handlePointerUp);
        window.addEventListener("pointercancel", handlePointerUp);
        return () => {
            window.removeEventListener("pointermove", handlePointerMove);
            window.removeEventListener("pointerup", handlePointerUp);
            window.removeEventListener("pointercancel", handlePointerUp);
            document.body.style.cursor = "";
        };
    }, []);

    useEffect(() => {
        const container = containerRef.current;
        if (!container) return;

        const preventWheelScroll = (event: WheelEvent) => {
            const target = event.target instanceof Element ? event.target : null;
            if (target?.closest("[data-canvas-no-zoom],.ant-modal,.ant-popover,.ant-dropdown,.ant-select-dropdown,.ant-picker-dropdown")) return;
            event.preventDefault();
        };
        container.addEventListener("wheel", preventWheelScroll, { passive: false });
        return () => container.removeEventListener("wheel", preventWheelScroll);
    }, [containerRef]);

    const temporaryTool = isSpacePressed;
    const activeTool = temporaryTool ? (tool === "select" ? "pan" : "select") : tool;
    const cursor = isPanning ? "grabbing" : activeTool === "pan" ? "grab" : undefined;

    return (
        <div
            ref={containerRef}
            className="relative h-full w-full select-none overflow-hidden"
            style={{ background: theme.canvas.background, cursor }}
            onPointerDown={handlePointerDown}
            onDoubleClick={handleDoubleClick}
            onWheel={handleWheel}
            onContextMenu={onContextMenu}
            onDragOver={(event) => event.preventDefault()}
            onDrop={onDrop}
        >
            {backgroundMode === "blank" ? null : (
                <div
                    data-canvas-grid
                    data-mode={backgroundMode}
                    data-line={theme.canvas.line}
                    data-dot={theme.canvas.dot}
                    className="pointer-events-none absolute inset-0 opacity-40"
                />
            )}
            <div data-canvas-world className="absolute origin-top-left">
                {children}
            </div>
        </div>
    );
}
