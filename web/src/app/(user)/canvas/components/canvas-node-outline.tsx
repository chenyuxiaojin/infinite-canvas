"use client";

import { useEffect, useId, useMemo, useRef, useState, type CSSProperties } from "react";
import { Input, Select } from "antd";
import { Clock, BookOpen, Box, Check, ChevronRight, CircleAlert, Clapperboard, FileText, Group, Image as ImageIcon, Layers, LoaderCircle, MapPin, Music2, Palette, Search, Settings2, Type, Users, Video, type LucideIcon } from "lucide-react";

import { canvasThemes } from "@/lib/canvas-theme";
import { cn } from "@/lib/utils";
import { useThemeStore } from "@/stores/use-theme-store";

import { CanvasNodeType, type CanvasNodeData } from "../types";

const NODE_TYPES = {
    [CanvasNodeType.Image]: { label: "图片", icon: ImageIcon },
    [CanvasNodeType.Panorama]: { label: "全景图", icon: ImageIcon },
    [CanvasNodeType.Video]: { label: "视频", icon: Video },
    [CanvasNodeType.Audio]: { label: "音频", icon: Music2 },
    [CanvasNodeType.Text]: { label: "文本", icon: Type },
    [CanvasNodeType.Config]: { label: "生成配置", icon: Settings2 },
    [CanvasNodeType.Director]: { label: "导演台", icon: Clapperboard },
    [CanvasNodeType.Group]: { label: "组", icon: Group },
};
const NODE_FILTER_OPTIONS = [{ label: "全部", value: "all" }, ...Object.entries(NODE_TYPES).map(([value, { label }]) => ({ label, value }))];
const NODE_STATUS = {
    pending_approval: { label: "待确认", icon: Clock },
    success: { label: "已完成", icon: Check },
    loading: { label: "生成中", icon: LoaderCircle },
    error: { label: "生成失败", icon: CircleAlert },
};

type NodeCategory = "story" | "characters" | "scenes" | "props" | "shots" | "other";
type OutlineSection = { id: string; label: string; icon: LucideIcon; categories: NodeCategory[]; children?: OutlineSection[] };
const OUTLINE_SECTIONS: OutlineSection[] = [
    { id: "story", label: "故事设定", icon: BookOpen, categories: ["story"] },
    {
        id: "visual", label: "视觉设定", icon: Palette, categories: ["characters", "scenes", "props"], children: [
            { id: "characters", label: "人物", icon: Users, categories: ["characters"] },
            { id: "scenes", label: "场景", icon: MapPin, categories: ["scenes"] },
            { id: "props", label: "道具", icon: Box, categories: ["props"] },
        ],
    },
    { id: "shots", label: "分集分镜", icon: Clapperboard, categories: ["shots"] },
    { id: "other", label: "其他节点", icon: Layers, categories: ["other"] },
];

// 只整理目录的显示顺序，不改写节点、原有画布分组或连线。
function getNodeCategory(title: string): NodeCategory {
    if (/分镜|\bEP\s*\d+\b|第[\d一二三四五六七八九十百]+集/i.test(title)) return "shots";
    if (/道具/.test(title)) return "props";
    if (/场景/.test(title)) return "scenes";
    if (/人物|角色|主角|男[一二三]号|女[一二三]号|定妆|三视图/.test(title)) return "characters";
    if (/案例|总控|故事|剧本|大纲|全局|风格|质感母词/.test(title)) return "story";
    return "other";
}

export function CanvasNodeOutline({ nodes, selectedNodeIds, onFocusNode, spotlightGroupId, onFocusGroup }: { nodes: CanvasNodeData[]; selectedNodeIds: Set<string>; onFocusNode: (nodeId: string) => void; spotlightGroupId?: string | null; onFocusGroup?: (groupId: string | null) => void }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [keyword, setKeyword] = useState("");
    const [typeFilter, setTypeFilter] = useState("all");
    const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
    const rowRefs = useRef(new Map<string, HTMLButtonElement>());
    const outlineId = useId();
    const filtering = Boolean(keyword.trim()) || typeFilter !== "all";

    const { groups, count } = useMemo(() => {
        const groups: Record<NodeCategory, CanvasNodeData[]> = { story: [], characters: [], scenes: [], props: [], shots: [], other: [] };
        const query = keyword.trim().toLowerCase();
        let count = 0;
        for (const node of nodes) {
            if (typeFilter !== "all" && node.type !== typeFilter) continue;
            const content = node.type === CanvasNodeType.Text ? node.metadata?.content : "";
            if (query && ![node.title, NODE_TYPES[node.type]?.label, content, node.metadata?.prompt].filter(Boolean).join(" ").toLowerCase().includes(query)) continue;
            groups[getNodeCategory(node.title)].push(node);
            count += 1;
        }
        return { groups, count };
    }, [nodes, keyword, typeFilter]);

    const selectedId = Array.from(selectedNodeIds)[0];
    const selectedNode = nodes.find((node) => node.id === selectedId);
    const selectedCategory = selectedNode ? getNodeCategory(selectedNode.title) : undefined;

    useEffect(() => {
        if (!selectedId || !selectedCategory) return;
        const ancestors = [selectedCategory, ...(["characters", "scenes", "props"].includes(selectedCategory) ? ["visual"] : [])];
        setCollapsed((current) => {
            if (!ancestors.some((id) => current.has(id))) return current;
            const next = new Set(current);
            ancestors.forEach((id) => next.delete(id));
            return next;
        });
    }, [selectedId, selectedCategory]);

    useEffect(() => {
        if (!selectedId) return;
        const frame = window.requestAnimationFrame(() => rowRefs.current.get(selectedId)?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
        return () => window.cancelAnimationFrame(frame);
    }, [selectedId, selectedCategory, collapsed, keyword, typeFilter]);

    function renderNodes(items: CanvasNodeData[]) {
        return (
            <ul className="space-y-0.5">
                {items.map((node) => {
                    const nodeType = NODE_TYPES[node.type];
                    const Icon = nodeType?.icon || FileText;
                    const active = selectedNodeIds.has(node.id);
                    const title = node.title || nodeType?.label || "未命名节点";
                    const status = node.metadata?.status && node.metadata.status !== "idle" ? NODE_STATUS[node.metadata.status] : undefined;
                    const StatusIcon = status?.icon;
                    return (
                        <li key={node.id} className="flex items-center">
                            <button
                                ref={(element) => { if (element) rowRefs.current.set(node.id, element); else rowRefs.current.delete(node.id); }}
                                type="button"
                                onClick={() => onFocusNode(node.id)}
                                aria-pressed={active}
                                title={[title, nodeType?.label, status?.label].filter(Boolean).join(" · ")}
                                className={cn("relative flex w-full items-start gap-2 rounded px-2.5 py-2 text-left transition-colors focus-visible:outline-2 focus-visible:outline-offset-[-2px]", !active && "hover:bg-[var(--outline-hover)]")}
                                style={{ background: active ? theme.toolbar.activeBg : undefined, color: active ? theme.toolbar.activeText : theme.node.text, outlineColor: theme.node.activeStroke }}
                            >
                                {active ? <span aria-hidden className="absolute inset-y-2 left-0 w-0.5 rounded-full" style={{ background: theme.node.activeStroke }} /> : null}
                                <Icon aria-hidden className="mt-0.5 size-3.5 shrink-0" style={{ color: theme.node.faint }} />
                                <span className={cn("min-w-0 flex-1 line-clamp-2 break-words text-sm leading-5", active && "font-medium")}>{title}</span>
                                {status && StatusIcon ? <span className="mt-0.5 shrink-0" role="img" aria-label={status.label} title={status.label} style={{ color: theme.node.muted }}><StatusIcon className={cn("size-3.5", node.metadata?.status === "loading" && "animate-spin motion-reduce:animate-none")} aria-hidden /></span> : null}
                            </button>
                            {node.type === "group" && onFocusGroup ? <button type="button" aria-pressed={spotlightGroupId === node.id} title={spotlightGroupId === node.id ? "退出场次聚焦" : "聚焦此场次"} className="shrink-0 px-2 py-2 text-xs" style={{ color: spotlightGroupId === node.id ? theme.toolbar.activeText : theme.node.muted }} onClick={() => onFocusGroup(spotlightGroupId === node.id ? null : node.id)}>{spotlightGroupId === node.id ? "退出" : "聚焦"}</button> : null}
                        </li>
                    );
                })}
            </ul>
        );
    }

    function renderSection(section: OutlineSection, nested = false) {
        const sectionCount = section.categories.reduce((total, category) => total + groups[category].length, 0);
        if (!sectionCount) return null;
        const open = filtering || !collapsed.has(section.id);
        const Icon = section.icon;
        const contentId = `${outlineId}-${section.id}`;
        return (
            <section key={section.id} className={nested ? "py-0.5" : "border-b py-2 last:border-b-0"} style={{ borderColor: theme.toolbar.border }}>
                <button
                    type="button"
                    aria-expanded={open}
                    aria-controls={contentId}
                    disabled={filtering}
                    title={filtering ? "筛选时自动展开" : undefined}
                    onClick={() => setCollapsed((current) => { const next = new Set(current); if (next.has(section.id)) next.delete(section.id); else next.add(section.id); return next; })}
                    className="flex w-full items-center gap-2 rounded px-1.5 py-2 text-left text-sm hover:bg-[var(--outline-hover)] focus-visible:outline-2 focus-visible:outline-offset-[-2px]"
                    style={{ color: theme.node.muted, outlineColor: theme.node.activeStroke }}
                >
                    <ChevronRight aria-hidden className={cn("size-3 shrink-0 transition-transform motion-reduce:transition-none", open && "rotate-90")} />
                    <Icon aria-hidden className="size-3.5 shrink-0" />
                    <span className="min-w-0 flex-1 font-medium">{section.label}</span>
                    <span className="pr-1 text-xs tabular-nums" style={{ color: theme.node.faint }}>{sectionCount}</span>
                </button>
                <div id={contentId} hidden={!open} className="ml-5 border-l pl-1.5" style={{ borderColor: theme.toolbar.border }}>
                    {open ? (section.children ? section.children.map((child) => renderSection(child, true)) : renderNodes(groups[section.categories[0]])) : null}
                </div>
            </section>
        );
    }

    return (
        <div className="flex h-full flex-col" style={{ "--outline-hover": theme.toolbar.itemHover } as CSSProperties}>
            <div className="flex items-center gap-2 px-4 pb-2 pt-1">
                <span className="text-xs font-medium" style={{ color: theme.node.muted }}>画布元素</span>
                <span className="text-xs tabular-nums" style={{ color: theme.node.faint }} aria-live="polite">{filtering ? `${count} / ${nodes.length}` : nodes.length}</span>
                <Select aria-label="按节点类型筛选" size="small" variant="borderless" className="ml-auto w-auto" popupMatchSelectWidth={false} value={typeFilter} onChange={setTypeFilter} options={NODE_FILTER_OPTIONS} />
            </div>
            <div className="px-3 pb-2">
                <Input aria-label="搜索节点" allowClear prefix={<Search aria-hidden className="size-3.5" style={{ color: theme.node.faint }} />} placeholder="搜索节点" value={keyword} onChange={(event) => setKeyword(event.target.value)} />
            </div>
            <nav aria-label="画布创作目录" className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pb-4">
                {count ? OUTLINE_SECTIONS.map((section) => renderSection(section)) : (
                    <div className="px-2 pt-12 text-center text-sm" style={{ color: theme.node.muted }}>
                        <p>{nodes.length ? "没有匹配的节点" : "画布暂无节点"}</p>
                        {nodes.length ? <button type="button" onClick={() => { setKeyword(""); setTypeFilter("all"); }} className="mt-3 rounded px-2 py-1 underline underline-offset-4 hover:bg-[var(--outline-hover)]">清除筛选</button> : null}
                    </div>
                )}
            </nav>
        </div>
    );
}
