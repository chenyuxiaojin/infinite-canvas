"use client";

import { Check, Search } from "lucide-react";
import { type UIEvent, useEffect, useState } from "react";
import { Alert, App, Button, Empty, Input, Modal, Segmented, Spin, Tag } from "antd";

import { ALL_PROMPTS_OPTION, type Prompt } from "@/services/api/prompts";
import { cn } from "@/lib/utils";
import { PromptCard } from "./prompt-card";
import { usePromptList } from "./use-prompt-list";
import { usePromptActions } from "./use-prompt-actions";
import { PromptDetailDialog } from "./prompt-detail-dialog";
import { useCopyText } from "@/hooks/use-copy-text";

export function PromptSelectDialog({ open, onOpenChange, onSelect }: { open: boolean; onOpenChange: (open: boolean) => void; onSelect: (prompt: string) => void }) {
    const { message } = App.useApp();
    const [keyword, setKeyword] = useState("");
    const [selectedTags, setSelectedTags] = useState<string[]>([]);
    const [selectedCategory, setSelectedCategory] = useState(ALL_PROMPTS_OPTION);
    const [favorites, setFavorites] = useState(false);
    const [detail, setDetail] = useState<Prompt | null>(null);
    const actions = usePromptActions(open);
    const copyText = useCopyText();
    useEffect(() => { if (!open) setDetail(null); }, [open]);
    const { query, items, tags: promptTags, categories: promptCategories } = usePromptList({ keyword, tags: selectedTags, category: selectedCategory, enabled: open, favorites });
    const toggleTag = (tag: string) => {
        if (tag === ALL_PROMPTS_OPTION) return setSelectedTags([]);
        setSelectedTags((items) => (items.includes(tag) ? items.filter((item) => item !== tag) : [...items, tag]));
    };
    const selectPrompt = (item: Prompt) => {
        void actions.run(item, (loaded) => { onSelect(loaded.prompt); onOpenChange(false); });
    };

    useEffect(() => {
        if (query.isError) message.error(query.error instanceof Error ? query.error.message : "获取提示词失败");
    }, [message, query.error, query.isError]);

    const handleListScroll = (event: UIEvent<HTMLDivElement>) => {
        const target = event.currentTarget;
        if (query.hasNextPage && !query.isFetchingNextPage && target.scrollTop + target.clientHeight >= target.scrollHeight - 160) void query.fetchNextPage();
    };

    return (
        <Modal title="提示词库" open={open} onCancel={() => onOpenChange(false)} footer={null} width={1040} centered>
            <div data-canvas-no-zoom onWheelCapture={(event) => event.stopPropagation()}>
                <div className="mb-4"><Segmented options={[{ label: "目录", value: "catalog" }, { label: "本机收藏", value: "favorites" }]} value={favorites ? "favorites" : "catalog"} onChange={(value) => { setFavorites(value === "favorites"); setSelectedCategory(ALL_PROMPTS_OPTION); setSelectedTags([]); }} /></div>
                <div className="mx-auto max-w-2xl">
                    <Input size="large" prefix={<Search className="size-4 text-stone-400" />} value={keyword} onChange={(event) => setKeyword(event.target.value)} placeholder="按标题查询" />
                </div>
                <div className="mt-5 grid gap-3">
                    <div className="grid gap-2 sm:grid-cols-[56px_minmax(0,1fr)] sm:items-start">
                        <div className="pt-2 text-xs font-medium text-stone-500 dark:text-stone-400">分类</div>
                        <div className="flex flex-wrap gap-2">
                            {promptCategories.map((category) => (
                                <Tag.CheckableTag key={category} checked={selectedCategory === category} className={cn("prompt-filter-tag", selectedCategory === category && "is-active")} onChange={() => setSelectedCategory(category)}>
                                    {category}
                                </Tag.CheckableTag>
                            ))}
                        </div>
                    </div>
                    <div className="grid gap-2 sm:grid-cols-[56px_minmax(0,1fr)] sm:items-start">
                        <div className="pt-2 text-xs font-medium text-stone-500 dark:text-stone-400">标签</div>
                        <div className="flex max-h-24 flex-wrap gap-2 overflow-y-auto">
                            {promptTags.map((tag) => {
                                const active = tag === ALL_PROMPTS_OPTION ? selectedTags.length === 0 : selectedTags.includes(tag);
                                return (
                                    <Tag.CheckableTag key={tag} checked={active} className={cn("prompt-filter-tag", active && "is-active")} onChange={() => toggleTag(tag)}>
                                        {tag}
                                    </Tag.CheckableTag>
                                );
                            })}
                        </div>
                    </div>
                </div>
                <div className="thin-scrollbar mt-6 max-h-[520px] overflow-y-auto pr-2" data-canvas-no-zoom onScroll={handleListScroll} onWheelCapture={(event) => event.stopPropagation()}>
                    {query.isLoading ? (
                        <div className="flex h-40 items-center justify-center">
                            <Spin />
                        </div>
                    ) : null}
                    <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
                        {items.map((item) => (
                            <PromptCard key={item.id} item={item} onOpen={() => setDetail(item)} onCopy={() => selectPrompt(item)} loading={actions.loadingId === item.id} actionLabel="加载并使用" actionIcon={<Check className="size-3.5" />} actionType="primary" />
                        ))}
                    </div>
                    {query.isError ? <Alert type="error" title="目录读取失败，不代表内容为空" action={<Button onClick={() => void query.refetch()}>重试</Button>} /> : !query.isLoading && items.length === 0 ? <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有找到匹配的提示词" className="py-8" /> : null}
                    {query.isFetchingNextPage ? (
                        <div className="py-4 text-center">
                            <Spin size="small" />
                        </div>
                    ) : null}
                </div>
            </div>
            <PromptDetailDialog prompt={open ? detail : null} onClose={() => setDetail(null)} onCopy={(text) => copyText(text, "提示词已复制")} />
        </Modal>
    );
}
