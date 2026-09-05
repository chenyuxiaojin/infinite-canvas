"use client";

import { useEffect, useState } from "react";
import { Copy, ExternalLink, Star } from "lucide-react";
import { Alert, Button, Modal, Space, Spin, Tag } from "antd";
import { fetchPromptDetail, type Prompt } from "@/services/api/prompts";
import { usePromptActions } from "./use-prompt-actions";

export function PromptDetailDialog({ prompt, onClose, onCopy }: { prompt: Prompt | null; onClose: () => void; onCopy: (prompt: string) => void }) {
    const [detail, setDetail] = useState<Prompt | null>(null);
    const [error, setError] = useState("");
    const [attempt, setAttempt] = useState(0);
    const actions = usePromptActions(Boolean(prompt));
    useEffect(() => {
        setDetail(null);
        setError("");
        if (!prompt) return;
        if (prompt.prompt) { setDetail(prompt); return; }
        const controller = new AbortController();
        void fetchPromptDetail(prompt.id, controller.signal).then((loaded) => {
            if (!controller.signal.aborted) setDetail(loaded);
        }).catch((reason) => {
            if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : "加载失败");
        });
        return () => controller.abort();
    }, [prompt, attempt]);
    const item = detail?.id === prompt?.id ? detail : null;
    return (
        <Modal title={prompt?.title} open={Boolean(prompt)} onCancel={onClose} footer={null} width={860} destroyOnHidden>
            {prompt ? <div className="grid gap-5 md:grid-cols-[280px_minmax(0,1fr)]">
                <div className="space-y-3">
                    {prompt.coverUrl ? <img src={prompt.coverUrl} alt={prompt.title} referrerPolicy="no-referrer" className="aspect-[4/3] w-full rounded-lg object-cover" /> : null}
                    <div className="flex flex-wrap gap-1.5">{prompt.tags.map((tag) => <Tag key={tag}>{tag}</Tag>)}</div>
                    <p className="text-xs leading-5 opacity-60">{item?.saved ? "已收藏到本机，断网也能读取全文。" : prompt.remote ? "这次加载仅供查看和使用，不会自动收藏或保存全文。" : "这是原来已有的本地提示词。"}</p>
                    {/^https?:\/\//.test(prompt.githubUrl) ? <a href={prompt.githubUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-xs"><ExternalLink className="size-3" />查看来源</a> : null}
                </div>
                <div className="min-w-0">
                    {error ? <Alert type="error" showIcon title={error} action={<Button size="small" onClick={() => setAttempt((value) => value + 1)}>重新加载</Button>} /> : !item ? <div className="flex items-center gap-3 py-12"><Spin /><span>正在按需加载全文…</span></div> : <>
                        <pre className="max-h-[55vh] overflow-auto whitespace-pre-wrap break-words font-sans text-sm leading-7">{item.prompt}</pre>
                        <Space wrap className="mt-5">
                            <Button type="primary" icon={<Copy className="size-4" />} onClick={() => onCopy(item.prompt)}>复制提示词</Button>
                            <Button disabled={item.saved} loading={actions.loadingId === item.id} icon={<Star className="size-4" />} onClick={async () => {
                                if (await actions.save(item)) setDetail({ ...item, saved: true });
                            }}>{item.saved ? "已收藏到本机" : "收藏到本机"}</Button>
                        </Space>
                    </>}
                </div>
            </div> : null}
        </Modal>
    );
}
