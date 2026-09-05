"use client";

import { useMemo } from "react";
import { AlertTriangle, Bot, CheckCircle2, FlaskConical, History, LoaderCircle, RotateCcw, ShieldAlert } from "lucide-react";
import { Button, Popover, Tooltip } from "antd";

import { canvasThemes } from "@/lib/canvas-theme";
import { useThemeStore } from "@/stores/use-theme-store";
import type { CanvasAgentChangeBatch, CanvasCollaborationState, CanvasNodeData } from "../types";

type CanvasCollaborationStatusProps = {
    collaboration: CanvasCollaborationState;
    nodes: CanvasNodeData[];
    onUndoLatest: () => void;
    onRunDemo?: () => void;
};

const statusLabels = {
    idle: "Agent 待命",
    running: "Agent 正在执行",
    success: "Agent 已完成",
    error: "Agent 执行失败",
    conflict: "Agent 需要检查",
} as const;

export function CanvasCollaborationStatus({ collaboration, nodes, onUndoLatest, onRunDemo }: CanvasCollaborationStatusProps) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const StatusIcon =
        collaboration.status.state === "running" ? LoaderCircle : collaboration.status.state === "success" ? CheckCircle2 : collaboration.status.state === "error" ? AlertTriangle : collaboration.status.state === "conflict" ? ShieldAlert : Bot;

    const history = <CanvasCollaborationHistory collaboration={collaboration} nodes={nodes} onUndoLatest={onUndoLatest} onRunDemo={onRunDemo} />;

    return (
        <div className="pointer-events-auto absolute left-1/2 top-4 z-[80] -translate-x-1/2" data-canvas-no-zoom>
            <Popover content={history} trigger="click" placement="bottom">
                <button
                    type="button"
                    className="flex h-9 max-w-[min(460px,55vw)] items-center gap-2 rounded-lg px-3 text-xs font-medium transition-colors"
                    style={{ color: theme.node.text, background: collaboration.status.state === "idle" ? "transparent" : theme.toolbar.panel }}
                    aria-label={`${statusLabels[collaboration.status.state]}，画布 revision ${collaboration.revision}，查看 Agent 变更历史`}
                    title={collaboration.status.message}
                >
                    <span className={collaboration.status.state === "running" ? "animate-spin" : ""}>
                        <StatusIcon className="size-4" aria-hidden="true" />
                    </span>
                    <span className="truncate">{statusLabels[collaboration.status.state]}</span>
                    <span className="shrink-0 font-mono text-[10px] opacity-60">rev {collaboration.revision}</span>
                </button>
            </Popover>
            <span className="sr-only" aria-live="polite">
                {collaboration.status.message}
            </span>
        </div>
    );
}

export function CanvasCollaborationHistory({ collaboration, nodes, onUndoLatest, onRunDemo }: CanvasCollaborationStatusProps) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const nodeTitleById = useMemo(() => new Map(nodes.map((node) => [node.id, node.title || "未命名节点"])), [nodes]);
    const latestReversible = collaboration.batches.findLast((batch) => batch.reversible && !batch.undoneAt);
    const undoBlocked = Boolean(latestReversible && !latestReversible.canUndoNow);
    const visibleBatches = collaboration.batches.slice(-8).reverse();
    return (
        <div className="w-[min(390px,calc(100vw-32px))]" style={{ color: theme.node.text }} data-testid="agent-batch-history">
            <div className="flex items-start justify-between gap-4 px-1 pb-3">
                <div>
                    <div className="flex items-center gap-2 text-sm font-semibold">
                        <History className="size-4" />
                        Agent 变更批次
                    </div>
                    <p className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>
                        当前画布 revision {collaboration.revision}，人工修改与 Agent 共用同一份节点状态。
                    </p>
                </div>
                <Tooltip title={undoBlocked ? "画布在该批次后已被修改，为保护人工结果不能直接撤销" : latestReversible ? "撤销最近一个可逆 Agent 批次" : "暂无可撤销批次"}>
                    <Button type="text" size="small" icon={<RotateCcw className="size-3.5" />} disabled={!latestReversible || undoBlocked} onClick={onUndoLatest}>
                        撤销批次
                    </Button>
                </Tooltip>
            </div>
            <div className="max-h-[min(420px,60vh)] space-y-2 overflow-y-auto pr-1">
                {visibleBatches.length ? (
                    visibleBatches.map((batch) => <BatchItem key={batch.id} batch={batch} nodeTitleById={nodeTitleById} />)
                ) : (
                    <div className="rounded-lg px-3 py-5 text-center text-xs" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}>
                        Agent 完成修改后，操作者、时间、摘要和影响节点会显示在这里。
                    </div>
                )}
            </div>
            {onRunDemo ? (
                <Button className="mt-3 w-full" type="dashed" icon={<FlaskConical className="size-4" />} disabled={collaboration.status.state === "running"} onClick={onRunDemo}>
                    运行零付费本地协作演示
                </Button>
            ) : null}
        </div>
    );
}

function BatchItem({ batch, nodeTitleById }: { batch: CanvasAgentChangeBatch; nodeTitleById: Map<string, string> }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const status = batch.undoneAt ? "已撤销" : batch.status === "success" ? (batch.reversible ? (batch.canUndoNow ? "完成 · 可撤销" : "完成 · 已有后续修改") : "完成 · 不可逆") : batch.status === "conflict" ? "冲突" : "失败";
    const nodeTitles = batch.affectedNodeIds.map((id) => nodeTitleById.get(id) || batch.affectedNodeTitles?.[id] || id).slice(0, 4);
    return (
        <article className="rounded-lg border px-3 py-2.5" style={{ borderColor: theme.toolbar.border, background: theme.node.panel }}>
            <div className="flex items-center justify-between gap-3 text-[11px]" style={{ color: theme.node.muted }}>
                <span>{batch.actor}</span>
                <span>{formatBatchTime(batch.completedAt)}</span>
            </div>
            <div className="mt-1.5 flex items-start justify-between gap-3">
                <p className="min-w-0 text-sm font-medium leading-5">{batch.summary}</p>
                <span className="shrink-0 rounded px-1.5 py-0.5 text-[10px]" style={{ background: theme.toolbar.itemHover, color: theme.node.text }}>
                    {status}
                </span>
            </div>
            <p className="mt-1.5 text-xs leading-5" style={{ color: theme.node.muted }}>
                影响 {batch.affectedNodeIds.length} 个节点
                {nodeTitles.length ? `：${nodeTitles.join("、")}${batch.affectedNodeIds.length > nodeTitles.length ? "…" : ""}` : ""}
            </p>
            {batch.error ? (
                <p className="mt-1 text-xs leading-5" style={{ color: theme.node.text }}>
                    原因：{batch.error}
                </p>
            ) : null}
        </article>
    );
}

function formatBatchTime(value: string) {
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return "时间未知";
    return new Intl.DateTimeFormat("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
    }).format(date);
}
