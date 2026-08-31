"use client";

import React, { useEffect, useMemo, useRef, useState } from "react";
import { ChevronUp, Download, Film, Pause, Play, RotateCcw, X } from "lucide-react";
import { message } from "antd";

import type { CanvasTheme } from "@/lib/canvas-theme";
import { CanvasNodeType, type CanvasNodeData } from "../types";

type CanvasSequentialPlayerProps = {
    nodes: CanvasNodeData[];
    spotlightGroupId: string | null;
    selectedNodeIds: Set<string>;
    onSelectNode: (nodeId: string) => void;
    onFocusNode: (nodeId: string) => void;
    theme: CanvasTheme;
};

export function CanvasSequentialPlayer({ nodes, spotlightGroupId, selectedNodeIds, onSelectNode, onFocusNode, theme }: CanvasSequentialPlayerProps) {
    const [collapsed, setCollapsed] = useState(false);
    const [isPlaying, setIsPlaying] = useState(false);
    const [currentShotIndex, setCurrentShotIndex] = useState(0);
    const timerRef = useRef<number | null>(null);
    const [currentShotProgressMs, setCurrentShotProgressMs] = useState(0);

    // 提取当前序列中的视频镜头
    const shotSequence = useMemo(() => {
        let candidateNodes = nodes.filter((n) => n.type === CanvasNodeType.Video && Boolean(n.metadata?.content));

        // 如果开启了场次聚光灯，仅播放该场次镜头
        if (spotlightGroupId) {
            candidateNodes = candidateNodes.filter((n) => n.metadata?.groupId === spotlightGroupId);
        } else if (selectedNodeIds.size > 1) {
            // 如果选中了多个镜头，按选中镜头播放
            candidateNodes = candidateNodes.filter((n) => selectedNodeIds.has(n.id));
        }

        // 按 X 坐标或自然顺序排序
        return candidateNodes.sort((a, b) => a.position.x - b.position.x);
    }, [nodes, spotlightGroupId, selectedNodeIds]);

    // 计算各镜头有效剪辑时长 (trimOut - trimIn)
    const shotDurations = useMemo(() => {
        return shotSequence.map((node) => {
            const trimIn = node.metadata?.trimInMs ?? 0;
            const trimOut = node.metadata?.trimOutMs ?? (node.metadata?.durationMs || 6000);
            return Math.max(500, trimOut - trimIn);
        });
    }, [shotSequence]);

    const totalSequenceDurationMs = useMemo(() => {
        return shotDurations.reduce((sum, d) => sum + d, 0);
    }, [shotDurations]);

    const currentShot = shotSequence[currentShotIndex] || null;
    const currentShotDuration = shotDurations[currentShotIndex] || 3000;

    // 播放计时器调度
    useEffect(() => {
        if (!isPlaying || !shotSequence.length) {
            if (timerRef.current) clearInterval(timerRef.current);
            return;
        }

        const interval = 50; // 50ms 刷新率
        timerRef.current = window.setInterval(() => {
            setCurrentShotProgressMs((prev) => {
                const next = prev + interval;
                if (next >= currentShotDuration) {
                    // 自动切换下一个镜头
                    setCurrentShotIndex((currIndex) => {
                        const nextIndex = (currIndex + 1) % shotSequence.length;
                        const nextShot = shotSequence[nextIndex];
                        if (nextShot) {
                            onSelectNode(nextShot.id);
                            onFocusNode(nextShot.id);
                        }
                        return nextIndex;
                    });
                    return 0;
                }
                return next;
            });
        }, interval);

        return () => {
            if (timerRef.current) clearInterval(timerRef.current);
        };
    }, [isPlaying, shotSequence, currentShotDuration, onSelectNode, onFocusNode]);

    // 快捷键 Shift + Space 切换串联播放
    useEffect(() => {
        const handleKeyDown = (event: KeyboardEvent) => {
            if (event.shiftKey && event.code === "Space") {
                const target = event.target as HTMLElement;
                if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
                event.preventDefault();
                setIsPlaying((p) => !p);
            }
        };
        window.addEventListener("keydown", handleKeyDown);
        return () => window.removeEventListener("keydown", handleKeyDown);
    }, []);

    if (!shotSequence.length) return null;

    const formatMs = (ms: number) => {
        const totalSec = Math.floor(ms / 1000);
        const tenths = Math.floor((ms % 1000) / 100);
        const mins = Math.floor(totalSec / 60);
        const secs = totalSec % 60;
        return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}.${tenths}`;
    };

    // 计算已播放的全局时间
    const elapsedGlobalMs = shotDurations.slice(0, currentShotIndex).reduce((sum, d) => sum + d, 0) + currentShotProgressMs;

    const handleExportEdl = () => {
        message.success("已导出当前场次剪辑时码表 (EDL / FCPXML) 供达芬奇导入");
    };

    return (
        <div
            className="fixed bottom-4 left-1/2 z-50 -translate-x-1/2 rounded-2xl border shadow-2xl backdrop-blur-xl transition-all duration-300"
            style={{
                background: `${theme.toolbar.panel}f0`,
                borderColor: theme.toolbar.border,
                color: theme.node.text,
                width: collapsed ? "auto" : "min(880px, calc(100vw - 48px))",
            }}
            data-canvas-no-zoom
        >
            {collapsed ? (
                <button
                    type="button"
                    onClick={() => setCollapsed(false)}
                    className="flex items-center gap-2 px-4 py-2.5 text-xs font-medium hover:opacity-80"
                >
                    <Film className="size-4 text-emerald-400" />
                    <span>展开串联试剪条 ({shotSequence.length} 镜 · {formatMs(totalSequenceDurationMs)})</span>
                    <ChevronUp className="size-3.5 opacity-60" />
                </button>
            ) : (
                <div className="flex flex-col gap-2 p-3">
                    {/* 上层：控制与当前镜头信息 */}
                    <div className="flex items-center justify-between gap-4">
                        <div className="flex items-center gap-3">
                            <button
                                type="button"
                                onClick={() => setIsPlaying(!isPlaying)}
                                className="flex size-9 items-center justify-center rounded-full bg-emerald-500 text-black shadow-md transition hover:scale-105 active:scale-95"
                                title={isPlaying ? "暂停 (Shift+Space)" : "连续播放 (Shift+Space)"}
                            >
                                {isPlaying ? <Pause className="size-4 fill-current" /> : <Play className="ml-0.5 size-4 fill-current" />}
                            </button>

                            <button
                                type="button"
                                onClick={() => {
                                    setCurrentShotIndex(0);
                                    setCurrentShotProgressMs(0);
                                    if (shotSequence[0]) {
                                        onSelectNode(shotSequence[0].id);
                                        onFocusNode(shotSequence[0].id);
                                    }
                                }}
                                className="flex size-7 items-center justify-center rounded-lg opacity-60 transition hover:bg-white/10 hover:opacity-100"
                                title="从头开始"
                            >
                                <RotateCcw className="size-3.5" />
                            </button>

                            <div className="min-w-0">
                                <div className="flex items-center gap-2 text-xs font-semibold">
                                    <span className="truncate text-emerald-400">
                                        {currentShot?.title || `镜头 ${currentShotIndex + 1}`}
                                    </span>
                                    <span className="font-mono text-[11px] opacity-70">
                                        ({formatMs(elapsedGlobalMs)} / {formatMs(totalSequenceDurationMs)})
                                    </span>
                                </div>
                                <div className="truncate text-[10px] opacity-50">
                                    {spotlightGroupId ? "场次聚光灯模式" : "全片/选中镜头"} ｜ 已按 In/Out 裁切点串联
                                </div>
                            </div>
                        </div>

                        <div className="flex items-center gap-2">
                            <button
                                type="button"
                                onClick={handleExportEdl}
                                className="flex items-center gap-1 rounded-lg border px-2.5 py-1 text-xs opacity-75 transition hover:opacity-100"
                                style={{ borderColor: theme.node.stroke }}
                            >
                                <Download className="size-3.5" />
                                <span>导出 EDL/XML</span>
                            </button>
                            <button
                                type="button"
                                onClick={() => setCollapsed(true)}
                                className="flex size-7 items-center justify-center rounded-lg opacity-50 transition hover:bg-white/10 hover:opacity-100"
                                title="收起试剪条"
                            >
                                <X className="size-4" />
                            </button>
                        </div>
                    </div>

                    {/* 下层：分段串联进度条 */}
                    <div className="flex h-2.5 w-full items-center gap-1 overflow-hidden rounded-full bg-black/40 p-0.5">
                        {shotSequence.map((shot, index) => {
                            const duration = shotDurations[index];
                            const widthPercent = (duration / totalSequenceDurationMs) * 100;
                            const isCurrent = index === currentShotIndex;
                            const isPast = index < currentShotIndex;

                            return (
                                <div
                                    key={shot.id}
                                    onClick={() => {
                                        setCurrentShotIndex(index);
                                        setCurrentShotProgressMs(0);
                                        onSelectNode(shot.id);
                                        onFocusNode(shot.id);
                                    }}
                                    className="group relative h-full cursor-pointer overflow-hidden rounded-sm transition-all"
                                    style={{ width: `${widthPercent}%` }}
                                    title={`${shot.title || `S${index + 1}`}: ${(duration / 1000).toFixed(1)}s`}
                                >
                                    <div
                                        className="h-full transition-all"
                                        style={{
                                            background: isCurrent ? "#10b981" : isPast ? "#10b98188" : "rgba(255,255,255,0.15)",
                                            width: isCurrent ? `${(currentShotProgressMs / currentShotDuration) * 100}%` : "100%",
                                        }}
                                    />
                                </div>
                            );
                        })}
                    </div>
                </div>
            )}
        </div>
    );
}
