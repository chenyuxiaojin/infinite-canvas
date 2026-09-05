"use client";

import { Copy, FileText } from "lucide-react";
import type { ReactNode } from "react";
import { Button, Card, Tag } from "antd";

import { formatPromptDate, type Prompt } from "@/services/api/prompts";

export function PromptCard({
    item,
    onOpen,
    onCopy,
    actionLabel = "复制",
    actionIcon = <Copy className="size-3.5" />,
    actionType = "text",
    extraAction,
    loading = false,
}: {
    item: Prompt;
    onOpen: () => void;
    onCopy: () => void;
    actionLabel?: string;
    actionIcon?: ReactNode;
    actionType?: "text" | "primary";
    extraAction?: ReactNode;
    loading?: boolean;
}) {
    return (
        <Card
            hoverable
            className="overflow-hidden"
            styles={{ body: { padding: 0 } }}
            cover={
                <button type="button" className="block w-full text-left" onClick={onOpen}>
                    {item.coverUrl ? <img src={item.coverUrl} alt={item.title} loading="lazy" referrerPolicy="no-referrer" className="aspect-[4/3] w-full object-cover" /> : <span className="grid aspect-[4/3] w-full place-items-center opacity-40"><FileText className="size-12" /></span>}
                </button>
            }
        >
            <button type="button" className="block w-full text-left" onClick={onOpen}>
                <div className="p-4">
                    <div className="flex items-start justify-between gap-3">
                        <h2 className="line-clamp-1 text-sm font-semibold text-stone-950 dark:text-stone-100">{item.title}</h2>
                        <span className="shrink-0 text-xs text-stone-400 dark:text-stone-500">{formatPromptDate(item.updatedAt)}</span>
                    </div>
                    <p className="mt-2 line-clamp-3 text-xs leading-5 text-stone-600 dark:text-stone-400">{item.preview || (item.remote ? "在线条目 · 选中后加载全文" : "本地已有 · 选中后查看全文")}</p>
                    <div className="mt-2 text-xs opacity-50">{item.saved ? "已收藏到本机" : item.remote ? "未收藏 · 不保存全文" : "原有本地内容"}</div>
                    <div className="mt-3 flex flex-wrap gap-1.5">
                        {item.tags.map((tag) => (
                            <Tag key={tag} className="m-0 text-[11px]">
                                {tag}
                            </Tag>
                        ))}
                    </div>
                </div>
            </button>
            <div className="flex items-center gap-2 px-4 pb-4">
                <Button block={actionType === "primary"} type={actionType} size="small" loading={loading} icon={actionIcon} onClick={onCopy}>
                    {actionLabel}
                </Button>
                {extraAction}
            </div>
        </Card>
    );
}
