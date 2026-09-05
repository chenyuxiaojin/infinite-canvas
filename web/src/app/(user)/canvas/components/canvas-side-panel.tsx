"use client";

import { useStoredMediaSource } from "@/hooks/use-stored-media-source";
import { assetMediaReference } from "@/services/asset-media-reference";
import { memo, useEffect, useMemo, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Empty, Input, Spin } from "antd";
import { useQuery } from "@tanstack/react-query";
import { BookOpen, ChevronRight, Eye, FileText, Music2, Plus, Search } from "lucide-react";
import { motion } from "motion/react";

import { AssetFormModal } from "@/components/assets/asset-form-modal";
import { PromptDetailDialog } from "@/components/prompts/prompt-detail-dialog";
import { usePromptActions } from "@/components/prompts/use-prompt-actions";
import { useCopyText } from "@/hooks/use-copy-text";
import { canvasThemes, type CanvasTheme } from "@/lib/canvas-theme";
import { cn } from "@/lib/utils";
import { fetchPrompts, type Prompt } from "@/services/api/prompts";
import { useAssetStore, type Asset } from "@/stores/use-asset-store";
import { useThemeStore } from "@/stores/use-theme-store";

import type { CanvasNodeData } from "../types";
import { CanvasNodeOutline } from "./canvas-node-outline";
import type { InsertAssetPayload } from "./asset-picker-modal";

export const CANVAS_ASSET_DRAG_TYPE = "application/x-infinite-canvas-asset";

const PANEL_MOTION_SECONDS = 0.5;
const PANEL_EASE = [0.22, 1, 0.36, 1] as const;
const PANEL_MIN_WIDTH = 220;
const PANEL_MAX_WIDTH = 480;
const PROMPT_CACHE_TIME = 24 * 60 * 60 * 1000;

type PanelTab = "canvas" | "assets" | "prompts";

type Props = {
    nodes: CanvasNodeData[];
    selectedNodeIds: Set<string>;
    open: boolean;
    width: number;
    spotlightGroupId?: string | null;
    onWidthChange: (width: number) => void;
    onFocusNode: (nodeId: string) => void;
    onFocusGroup?: (groupId: string | null) => void;
    onAssetDragStart: (payload: InsertAssetPayload) => void;
    onAssetDragEnd: () => void;
    onInsertAsset: (payload: InsertAssetPayload) => void;
};

const ASSET_TYPE_OPTIONS = [
    { label: "全部", value: "" },
    { label: "文本", value: "text" },
    { label: "图片", value: "image" },
    { label: "视频", value: "video" },
    { label: "音频", value: "audio" },
];

export function CanvasSidePanel({ nodes, selectedNodeIds, open, width, spotlightGroupId, onWidthChange, onFocusNode, onFocusGroup, onAssetDragStart, onAssetDragEnd, onInsertAsset }: Props) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [tab, setTab] = useState<PanelTab>("canvas");
    const [mounted, setMounted] = useState(open);
    const [closing, setClosing] = useState(false);
    const [resizing, setResizing] = useState(false);

    useEffect(() => {
        if (open) {
            setMounted(true);
            setClosing(false);
            return;
        }
        setClosing(true);
        const timer = window.setTimeout(() => {
            setMounted(false);
            setClosing(false);
        }, PANEL_MOTION_SECONDS * 1000);
        return () => window.clearTimeout(timer);
    }, [open]);

    const startResize = (event: ReactPointerEvent<HTMLButtonElement>) => {
        event.preventDefault();
        const startX = event.clientX;
        const startWidth = width;
        const onMove = (moveEvent: PointerEvent) => onWidthChange(Math.min(PANEL_MAX_WIDTH, Math.max(PANEL_MIN_WIDTH, startWidth + moveEvent.clientX - startX)));
        const onUp = () => {
            window.removeEventListener("pointermove", onMove);
            window.removeEventListener("pointerup", onUp);
            setResizing(false);
        };
        setResizing(true);
        window.addEventListener("pointermove", onMove);
        window.addEventListener("pointerup", onUp);
    };

    if (!mounted) return null;

    return (
        <motion.div
            className="relative z-[60] flex h-full shrink-0"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: open ? width + 1 : 0, opacity: open ? 1 : 0 }}
            transition={{ duration: resizing ? 0 : PANEL_MOTION_SECONDS, ease: PANEL_EASE }}
            style={{ overflow: "clip", pointerEvents: closing ? "none" : undefined }}
        >
            <motion.aside
                className="relative flex h-full shrink-0 flex-col overflow-hidden border-r"
                initial={{ x: -48 }}
                animate={{ x: closing ? -28 : 0 }}
                transition={{ duration: resizing ? 0 : PANEL_MOTION_SECONDS, ease: PANEL_EASE }}
                style={{ width, background: theme.toolbar.panel, borderColor: theme.toolbar.border, color: theme.node.text }}
                data-canvas-no-zoom
            >
                <div className="flex items-center gap-5 px-4 pt-3.5">
                    <PanelTabButton label="画布" active={tab === "canvas"} theme={theme} onClick={() => setTab("canvas")} />
                    <PanelTabButton label="资产" active={tab === "assets"} theme={theme} onClick={() => setTab("assets")} />
                    <PanelTabButton label="提示词库" active={tab === "prompts"} theme={theme} onClick={() => setTab("prompts")} />
                </div>
                <div className="mt-2 min-h-0 flex-1 overflow-hidden">
                    {tab === "canvas" ? (
                        <CanvasNodeOutline nodes={nodes} selectedNodeIds={selectedNodeIds} onFocusNode={onFocusNode} spotlightGroupId={spotlightGroupId} onFocusGroup={onFocusGroup} />
                    ) : tab === "assets" ? (
                        <CanvasAssetsTab theme={theme} onAssetDragStart={onAssetDragStart} onAssetDragEnd={onAssetDragEnd} />
                    ) : (
                        <CanvasPromptsTab theme={theme} onInsert={onInsertAsset} />
                    )}
                </div>
                <button type="button" className="absolute inset-y-0 right-0 z-40 w-4 translate-x-1/2 cursor-col-resize" onPointerDown={startResize} aria-label="调整左侧面板宽度" />
            </motion.aside>
        </motion.div>
    );
}

function PanelTabButton({ label, active, theme, onClick }: { label: string; active: boolean; theme: CanvasTheme; onClick: () => void }) {
    return (
        <button type="button" onClick={onClick} className="relative pb-1.5 text-sm font-semibold transition-opacity" style={{ color: theme.node.text, opacity: active ? 1 : 0.45 }}>
            {label}
            {active ? <motion.span layoutId="sidePanelTabIndicator" className="absolute inset-x-0 -bottom-px h-0.5 rounded-full" style={{ background: theme.toolbar.activeText }} transition={{ type: "spring", stiffness: 500, damping: 34 }} /> : null}
        </button>
    );
}

const CanvasAssetsTab = memo(function CanvasAssetsTab({ theme, onAssetDragStart, onAssetDragEnd }: { theme: CanvasTheme; onAssetDragStart: (payload: InsertAssetPayload) => void; onAssetDragEnd: () => void }) {
    const [formOpen, setFormOpen] = useState(false);

    return (
        <div className="flex h-full flex-col">
            <MyAssetsTab theme={theme} onAdd={() => setFormOpen(true)} onAssetDragStart={onAssetDragStart} onAssetDragEnd={onAssetDragEnd} />
            <AssetFormModal open={formOpen} onClose={() => setFormOpen(false)} />
        </div>
    );
});

function AssetSourceTab({ label, active, theme, onClick }: { label: string; active: boolean; theme: CanvasTheme; onClick: () => void }) {
    return (
        <button type="button" onClick={onClick} className="relative pb-1 text-xs font-semibold transition-opacity" style={{ color: theme.node.text, opacity: active ? 1 : 0.45 }}>
            {label}
            {active ? <span className="absolute inset-x-0 -bottom-px h-0.5 rounded-full" style={{ background: theme.toolbar.activeText }} /> : null}
        </button>
    );
}

function MyAssetsTab({ theme, onAdd, onAssetDragStart, onAssetDragEnd }: { theme: CanvasTheme; onAdd: () => void; onAssetDragStart: (payload: InsertAssetPayload) => void; onAssetDragEnd: () => void }) {
    const assets = useAssetStore((state) => state.assets);
    const [keyword, setKeyword] = useState("");
    const [type, setType] = useState("");
    const filtered = useMemo(() => {
        const query = keyword.trim().toLowerCase();
        return assets.filter((asset) => (!type || asset.kind === type) && (!query || [asset.title, ...(asset.tags || [])].join(" ").toLowerCase().includes(query)));
    }, [assets, keyword, type]);

    return (
        <>
            <div className="flex items-center gap-4 px-3 pb-2">
                {ASSET_TYPE_OPTIONS.map((option) => <AssetSourceTab key={option.value || "all"} label={option.label} active={type === option.value} theme={theme} onClick={() => setType(option.value)} />)}
            </div>
            <div className="flex items-center gap-2 px-3 pb-2">
                <Input size="small" allowClear prefix={<Search className="size-3.5 text-stone-400" />} placeholder="搜索素材" value={keyword} onChange={(event) => setKeyword(event.target.value)} />
                <button type="button" onClick={onAdd} className="flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-xs font-semibold transition hover:bg-black/5 dark:hover:bg-white/10" style={{ color: theme.node.text }}>
                    <Plus className="size-3.5" />
                    添加
                </button>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
                {filtered.length ? <div className="grid grid-cols-2 gap-2 px-1 pt-1">{filtered.map((asset) => <AssetDragCard key={asset.id} asset={asset} theme={theme} onAssetDragStart={onAssetDragStart} onAssetDragEnd={onAssetDragEnd} />)}</div> : <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无素材" className="pt-16" />}
            </div>
        </>
    );
}

function AssetDragCard({ asset, theme, onAssetDragStart, onAssetDragEnd }: { asset: Asset; theme: CanvasTheme; onAssetDragStart: (payload: InsertAssetPayload) => void; onAssetDragEnd: () => void }) {
    const source = useStoredMediaSource({ ...assetMediaReference(asset, true), observe: true });
    return <div ref={source.ref}><DraggableAssetCard theme={theme} title={asset.title} payload={assetPayload(asset)} kind={asset.kind} imageUrl={source.src} text={asset.kind === "text" ? asset.data.content : ""} onAssetDragStart={onAssetDragStart} onAssetDragEnd={onAssetDragEnd} /></div>;
}

function DraggableAssetCard({ theme, title, payload, kind, imageUrl, text, onAssetDragStart, onAssetDragEnd }: { theme: CanvasTheme; title: string; payload: InsertAssetPayload; kind: "text" | "image" | "video" | "audio"; imageUrl: string; text: string; onAssetDragStart: (payload: InsertAssetPayload) => void; onAssetDragEnd: () => void }) {
    return (
        <div
            draggable
            title={title}
            onDragStart={(event) => {
                event.dataTransfer.setData(CANVAS_ASSET_DRAG_TYPE, "asset");
                event.dataTransfer.effectAllowed = "copy";
                onAssetDragStart(payload);
            }}
            onDragEnd={onAssetDragEnd}
            className="group relative aspect-square cursor-grab overflow-hidden rounded-xl border transition duration-200 hover:-translate-y-0.5 hover:shadow-lg active:cursor-grabbing"
            style={{ borderColor: theme.node.stroke, background: theme.node.panel }}
        >
            {kind === "text" ? imageUrl ? <div className="flex size-full flex-col"><img src={imageUrl} alt={title} className="h-1/2 w-full object-cover" /><div className="h-1/2 overflow-hidden whitespace-pre-wrap break-words p-2.5 text-[11px] leading-snug opacity-80">{text}</div></div> : <div className="size-full overflow-hidden whitespace-pre-wrap break-words p-2.5 text-[11px] leading-snug opacity-80">{text}</div> : kind === "audio" ? <span className="grid size-full place-items-center"><Music2 className="size-8 opacity-45" /></span> : imageUrl ? kind === "video" ? <video src={imageUrl + "#t=0.1"} muted playsInline preload="metadata" className="size-full object-cover transition duration-300 group-hover:scale-[1.04]" /> : <img src={imageUrl} alt={title} className="size-full object-cover transition duration-300 group-hover:scale-[1.04]" /> : <span className="grid size-full place-items-center"><FileText className="size-8 opacity-45" /></span>}
        </div>
    );
}

function assetPayload(asset: Asset): InsertAssetPayload {
    if (asset.kind === "text") return { kind: "text", content: asset.data.content, title: asset.title, assetId: asset.id, source: "asset" };
    if (asset.kind === "image") return { kind: "image", dataUrl: asset.data.dataUrl, storageKey: asset.data.storageKey, title: asset.title, assetId: asset.id, width: asset.data.width, height: asset.data.height, bytes: asset.data.bytes, mimeType: asset.data.mimeType, source: "asset" };
    if (asset.kind === "video") return { kind: "video", url: asset.data.url, storageKey: asset.data.storageKey, title: asset.title, assetId: asset.id, width: asset.data.width, height: asset.data.height, bytes: asset.data.bytes, mimeType: asset.data.mimeType, source: "asset" };
    return { kind: "audio", url: asset.data.url, storageKey: asset.data.storageKey, title: asset.title, assetId: asset.id, bytes: asset.data.bytes, mimeType: asset.data.mimeType, durationMs: asset.data.durationMs, source: "asset" };
}

const CanvasPromptsTab = memo(function CanvasPromptsTab({ theme, onInsert }: { theme: CanvasTheme; onInsert: (payload: InsertAssetPayload) => void }) {
    const copyText = useCopyText();
    const [keyword, setKeyword] = useState("");
    const [expanded, setExpanded] = useState<Record<string, boolean>>({ system: true });
    const [detail, setDetail] = useState<Prompt | null>(null);
    const categoryQuery = useQuery({
        queryKey: ["canvas-side-prompt-categories"],
        queryFn: () => fetchPrompts({ page: 1, pageSize: 1 }),
        retry: false,
    });
    const categories = useMemo(() => ["system", ...(categoryQuery.data?.categories.filter((category) => category !== "system") || [])], [categoryQuery.data?.categories]);

    return (
        <div className="flex h-full flex-col">
            <div className="px-3 pb-2.5 pt-1">
                <Input size="small" allowClear prefix={<Search className="size-3.5 text-stone-400" />} placeholder="搜索提示词" value={keyword} onChange={(event) => setKeyword(event.target.value)} />
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
                {categoryQuery.isLoading ? <div className="flex justify-center pt-16"><Spin size="small" /></div> : (
                    <div className="space-y-2">
                        {categories.map((category) => {
                            const opened = Boolean(expanded[category]) || Boolean(keyword.trim());
                            return <PromptGroup key={category} category={category} keyword={keyword} open={opened} theme={theme} onToggle={() => setExpanded((current) => ({ ...current, [category]: !current[category] }))} onView={setDetail} onInsert={onInsert} />;
                        })}
                    </div>
                )}
            </div>
            <PromptDetailDialog prompt={detail} onClose={() => setDetail(null)} onCopy={(prompt) => copyText(prompt, "已复制提示词")} />
        </div>
    );
});

async function fetchPromptCategory(category: string) {
    const first = await fetchPrompts({ category, page: 1, pageSize: 500 });
    if (first.total <= first.items.length) return first.items;

    const pages = await Promise.all(
        Array.from(
            { length: Math.ceil(first.total / 500) - 1 },
            (_, index) => fetchPrompts({ category, page: index + 2, pageSize: 500 }),
        ),
    );

    return [...first.items, ...pages.flatMap((page) => page.items)];
}

function PromptGroup({ category, keyword, open, theme, onToggle, onView, onInsert }: { category: string; keyword: string; open: boolean; theme: CanvasTheme; onToggle: () => void; onView: (prompt: Prompt) => void; onInsert: (payload: InsertAssetPayload) => void }) {
    const actions = usePromptActions();
    const label = category === "system" ? "系统提示词" : category;
    const query = useQuery({
        queryKey: ["canvas-side-prompt-category", category],
        queryFn: () => fetchPromptCategory(category),
        enabled: open,
        staleTime: PROMPT_CACHE_TIME,
        gcTime: PROMPT_CACHE_TIME,
        retry: false,
    });
    const items = useMemo(() => {
        const queryText = keyword.trim().toLowerCase();
        const cachedItems = query.data || [];
        return queryText ? cachedItems.filter((item) => [item.title, item.preview, ...item.tags].join(" ").toLowerCase().includes(queryText)) : cachedItems;
    }, [keyword, query.data]);
    return (
        <div>
            <button type="button" onClick={onToggle} className="flex w-full items-center gap-1.5 rounded-md px-1.5 py-1.5 text-left text-xs font-semibold opacity-75 transition hover:opacity-100">
                <ChevronRight className={cn("size-3.5 transition-transform", open && "rotate-90")} />
                <BookOpen className="size-3.5" />
                <span className="min-w-0 flex-1 truncate">{label}</span>
                {query.isSuccess && (category === "system" || open) ? <span className="opacity-50">{items.length}</span> : null}
            </button>
            {open ? (
                <div className="space-y-1.5 px-1 pb-2 pt-1">
                    {query.isLoading ? <div className="flex justify-center py-6"><Spin size="small" /></div> : query.isError ? (
                        <button type="button" onClick={() => void query.refetch()} className="block w-full py-4 text-center text-xs text-red-500 opacity-80 transition hover:opacity-100">
                            加载失败，点击重试
                        </button>
                    ) : items.length ? (
                        items.map((item) => <PromptRow key={item.id} item={item} theme={theme} loading={actions.loadingId === item.id} onView={() => onView(item)} onInsert={() => void actions.run(item, (loaded) => onInsert({ kind: "text", content: loaded.prompt, title: loaded.title }))} />)
                    ) : (
                        <div className="py-4 text-center text-xs opacity-40">{category === "system" ? "暂无提示词" : "该分类暂无提示词"}</div>
                    )}
                </div>
            ) : null}
        </div>
    );
}

function PromptRow({ item, theme, onView, onInsert, loading }: { item: Prompt; theme: CanvasTheme; onView: () => void; onInsert: () => void; loading: boolean }) {
    return (
        <div className="group relative flex items-center gap-2.5 rounded-lg px-2 py-2 transition hover:bg-black/5 dark:hover:bg-white/5">
            {item.coverUrl ? <img src={item.coverUrl} alt="" className="size-10 shrink-0 rounded-md object-cover" loading="lazy" /> : <span className="grid size-10 shrink-0 place-items-center rounded-md" style={{ background: theme.node.panel }}><FileText className="size-4 opacity-50" /></span>}
            <button type="button" onClick={onView} className="min-w-0 flex-1 text-left">
                <span className="block truncate text-sm font-medium leading-snug">{item.title}</span>
                <span className="mt-0.5 block truncate text-xs leading-snug opacity-50">{loading ? "正在加载全文…" : item.preview || "选中后加载全文"}</span>
            </button>
            <div className="flex shrink-0 flex-col items-center gap-0.5">
                <button type="button" onClick={onView} className="grid size-6 place-items-center rounded-md opacity-60 transition hover:bg-black/10 hover:opacity-100 dark:hover:bg-white/10" aria-label="查看详情"><Eye className="size-3.5" /></button>
                <button type="button" disabled={loading} onClick={onInsert} className="grid size-6 place-items-center rounded-md opacity-60 transition hover:bg-black/10 hover:opacity-100 dark:hover:bg-white/10" style={{ color: theme.toolbar.activeText }} aria-label="加载并插入画布"><Plus className="size-3.5" /></button>
            </div>
        </div>
    );
}
