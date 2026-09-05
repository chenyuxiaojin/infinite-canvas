"use client";

import { Eye, Star, Plus, RefreshCw, Search } from "lucide-react";
import { type UIEvent, useEffect, useState } from "react";
import { Alert, App, Button, Empty, Form, Input, Modal, Segmented, Select, Spin, Tag } from "antd";
import { useQueryClient } from "@tanstack/react-query";

import { PromptCard } from "@/components/prompts/prompt-card";
import { PromptDetailDialog } from "@/components/prompts/prompt-detail-dialog";
import { usePromptList } from "@/components/prompts/use-prompt-list";
import { useCopyText } from "@/hooks/use-copy-text";
import { cn } from "@/lib/utils";
import { usePromptActions } from "@/components/prompts/use-prompt-actions";
import { ALL_PROMPTS_OPTION, savePromptCategory, syncPrompts, type Prompt } from "@/services/api/prompts";

export default function PromptsPage() {
    const { message } = App.useApp();
    const [titleInput, setTitleInput] = useState("");
    const [titleKeyword, setTitleKeyword] = useState("");
    const [selectedTags, setSelectedTags] = useState<string[]>([]);
    const [selectedCategory, setSelectedCategory] = useState(ALL_PROMPTS_OPTION);
    const [selectedPrompt, setSelectedPrompt] = useState<Prompt | null>(null);
    const [syncing, setSyncing] = useState(false);
    const [syncErrors, setSyncErrors] = useState<string[]>([]);
    const [favorites, setFavorites] = useState(false);
    const [addModalOpen, setAddModalOpen] = useState(false);
    const [addForm] = Form.useForm();
    const actions = usePromptActions();
    const client = useQueryClient();
    const copyText = useCopyText();
    const { query, items: promptItems, tags: promptTags, categories: promptCategoryOptions, total: totalPrompts } = usePromptList({ keyword: titleKeyword, tags: selectedTags, category: selectedCategory, favorites });

    const handleSync = async () => {
        try {
            setSyncing(true);
            setSyncErrors([]);
            const results = await syncPrompts();
            const failed = results.filter((result) => result.error).map((result) => `${result.name}：${result.error}`);
            setSyncErrors(failed);
            if (failed.length) message.warning(`${results.length - failed.length} 个来源已更新，${failed.length} 个失败；原目录保留`);
            else message.success("目录已更新，未保存提示词全文");
            await client.invalidateQueries({ queryKey: ["prompts"] });
            await client.invalidateQueries({ queryKey: ["canvas-side-prompt-category"] });
        } catch (error) {
            message.error(error instanceof Error ? error.message : "同步提示词失败");
        } finally {
            setSyncing(false);
        }
    };

    const handleAddSource = async (values: { category: string; name: string; sourceType: string; pathOrUrl: string; description?: string }) => {
        try {
            await savePromptCategory({
                category: values.category.trim(),
                name: values.name.trim(),
                sourceType: values.sourceType,
                pathOrUrl: values.pathOrUrl.trim(),
                description: values.description || "",
                remote: values.sourceType !== "local_markdown",
                enabled: true,
            });
            message.success("来源已添加，正在读取目录（不保存全文）…");
            setAddModalOpen(false);
            addForm.resetFields();
            await syncPrompts(values.category.trim());
            void query.refetch();
        } catch (error) {
            message.error(error instanceof Error ? error.message : "添加订阅源失败");
        }
    };

    useEffect(() => {
        if (query.isError) {
            message.error(query.error instanceof Error ? query.error.message : "获取提示词失败");
        }
    }, [message, query.error, query.isError]);

    const toggleTag = (tag: string) => {
        if (tag === ALL_PROMPTS_OPTION) return setSelectedTags([]);
        setSelectedTags((items) => (items.includes(tag) ? items.filter((item) => item !== tag) : [...items, tag]));
    };

    const searchByTitleInput = () => {
        setTitleKeyword(titleInput);
    };

    const handleListScroll = (event: UIEvent<HTMLDivElement>) => {
        const target = event.currentTarget;
        if (query.hasNextPage && !query.isFetchingNextPage && target.scrollTop + target.clientHeight >= target.scrollHeight - 160) {
            void query.fetchNextPage();
        }
    };

    return (
        <div className="flex h-full flex-col overflow-hidden bg-background text-stone-800 dark:text-stone-100">
            <main
                className="min-h-0 flex-1 overflow-y-auto bg-background bg-[radial-gradient(#e5e7eb_1px,transparent_1px)] px-6 py-8 [background-size:16px_16px] dark:bg-[radial-gradient(rgba(245,245,244,.16)_1px,transparent_1px)]"
                onScroll={handleListScroll}
            >
                <div className="pb-8">
                    <div className="mx-auto max-w-5xl text-center">
                        <h1 className="text-4xl font-semibold tracking-tight text-stone-950 dark:text-stone-100">提示词中心</h1>
                        <p className="mt-3 text-sm text-stone-500 dark:text-stone-400">{query.isError ? "目录读取失败，不代表内容为空" : query.isLoading ? "正在读取目录…" : `共 ${totalPrompts} 条${favorites ? "本机收藏" : "目录条目"}`} · 搜索看预览，选中加载，主动收藏才保存。</p>
                        <div className="mt-5"><Segmented options={[{ label: "在线目录与原有内容", value: "catalog" }, { label: "我的本机收藏", value: "favorites" }]} value={favorites ? "favorites" : "catalog"} onChange={(value) => { setFavorites(value === "favorites"); setSelectedCategory(ALL_PROMPTS_OPTION); setSelectedTags([]); }} /></div>
                        <p className="mt-3 text-xs opacity-50">目录只保留标题、标签和预览链接；相同条目合并显示，旧库原样保留。</p>
                        <div className="mt-5 flex items-center justify-center gap-3">
                            <Button
                                type="primary"
                                icon={<RefreshCw className={cn("size-4", syncing && "animate-spin")} />}
                                loading={syncing}
                                onClick={handleSync}
                            >
                                更新在线目录
                            </Button>
                            <Button
                                icon={<Plus className="size-4" />}
                                onClick={() => setAddModalOpen(true)}
                            >
                                添加订阅源 (URL / 本地路径)
                            </Button>
                        </div>
                    </div>
                    {syncErrors.length ? <div className="mx-auto mt-4 max-w-5xl"><Alert type="warning" showIcon title="部分来源更新失败，已保留原目录" description={syncErrors.join("；")} /></div> : null}
                    {query.isError ? <div className="mx-auto mt-4 max-w-5xl"><Alert type="error" showIcon title="读取失败，现有内容没有被清空" description={query.error instanceof Error ? query.error.message : "请重试"} action={<Button onClick={() => void query.refetch()}>重新读取</Button>} /></div> : null}
                    {query.isLoading ? (
                        <div className="flex h-60 items-center justify-center">
                            <Spin />
                        </div>
                    ) : null}
                    {!query.isLoading ? (
                        <>
                            <div className="mx-auto mt-8 w-full max-w-2xl">
                                <Input size="large" className="w-full" prefix={<Search className="size-4 text-stone-400" />} value={titleInput} placeholder="搜索标题、标签或分类，按 Enter 搜索" onChange={(event) => setTitleInput(event.target.value)} onPressEnter={searchByTitleInput} />
                            </div>
                            <div className="mx-auto mt-6 grid max-w-6xl gap-3 text-left">
                                <div className="grid gap-2 sm:grid-cols-[56px_minmax(0,1fr)] sm:items-start">
                                    <div className="pt-2 text-xs font-medium text-stone-500 dark:text-stone-400">分类</div>
                                    <div className="flex flex-wrap gap-2">
                                        {promptCategoryOptions.map((category) => (
                                            <Tag.CheckableTag key={category} checked={selectedCategory === category} className={cn("prompt-filter-tag", selectedCategory === category && "is-active")} onChange={() => setSelectedCategory(category)}>
                                                {category}
                                            </Tag.CheckableTag>
                                        ))}
                                    </div>
                                </div>
                                <div className="grid gap-2 sm:grid-cols-[56px_minmax(0,1fr)] sm:items-start">
                                    <div className="pt-2 text-xs font-medium text-stone-500 dark:text-stone-400">标签</div>
                                    <div className="flex max-h-28 flex-wrap gap-2 overflow-y-auto">
                                        {promptTags.map((tag) => (
                                            <Tag.CheckableTag
                                                key={tag}
                                                checked={tag === ALL_PROMPTS_OPTION ? selectedTags.length === 0 : selectedTags.includes(tag)}
                                                className={cn("prompt-filter-tag", (tag === ALL_PROMPTS_OPTION ? selectedTags.length === 0 : selectedTags.includes(tag)) && "is-active")}
                                                onChange={() => toggleTag(tag)}
                                            >
                                                {tag}
                                            </Tag.CheckableTag>
                                        ))}
                                    </div>
                                </div>
                            </div>
                        </>
                    ) : null}
                </div>

                {!query.isLoading ? (
                    <div>
                        <div className="mx-auto grid max-w-7xl gap-5 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
                            {promptItems.map((item) => (
                                <PromptCard
                                    key={item.id}
                                    item={item}
                                    onOpen={() => setSelectedPrompt(item)}
                                    onCopy={() => setSelectedPrompt(item)}
                                    actionLabel="查看全文"
                                    actionIcon={<Eye className="size-3.5" />}
                                    loading={actions.loadingId === item.id}
                                    extraAction={
                                        <Button size="small" disabled={Boolean(actions.loadingId)} icon={<Star className="size-3.5" />} onClick={() => item.saved ? actions.remove(item) : void actions.save(item)}>
                                            {item.saved ? "取消收藏" : "收藏到本机"}
                                        </Button>
                                    }
                                />
                            ))}
                        </div>
                        {!query.isError && promptItems.length === 0 ? <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={favorites ? "还没有匹配的本机收藏" : "没有找到匹配的目录条目"} className="py-16" /> : null}
                        <div className="mx-auto mt-6 max-w-7xl text-center text-xs text-stone-500 dark:text-stone-400">
                            {query.isFetchingNextPage ? "加载中..." : query.hasNextPage ? "继续向下滚动加载更多" : promptItems.length > 0 ? "已经到底了" : null}
                        </div>
                    </div>
                ) : null}
            </main>

            <PromptDetailDialog prompt={selectedPrompt} onClose={() => setSelectedPrompt(null)} onCopy={(prompt) => copyText(prompt, "提示词已复制")} />

            <Modal
                title="添加提示词订阅源"
                open={addModalOpen}
                onCancel={() => setAddModalOpen(false)}
                onOk={() => addForm.submit()}
                okText="添加并读取目录"
                cancelText="取消"
            >
                <Form form={addForm} layout="vertical" onFinish={handleAddSource} initialValues={{ sourceType: "custom_url" }}>
                    <Form.Item name="category" label="分类唯一标识 (英文/拼音)" rules={[{ required: true, message: "请输入分类标识，如 custom-prompts" }]}>
                        <Input placeholder="custom-prompts" />
                    </Form.Item>
                    <Form.Item name="name" label="分类显示名称" rules={[{ required: true, message: "请输入显示名称" }]}>
                        <Input placeholder="我的专属分镜提示词" />
                    </Form.Item>
                    <Form.Item name="sourceType" label="源类型" rules={[{ required: true }]}>
                        <Select options={[
                            { value: "local_markdown", label: "本地 Markdown 文件 (绝对路径)" },
                            { value: "custom_url", label: "远端 Markdown URL（GitHub Raw）" },
                        ]} />
                    </Form.Item>
                    <Form.Item name="pathOrUrl" label="本地路径或网络 URL" rules={[{ required: true, message: "请输入本地文件绝对路径或网络 URL" }]}>
                        <Input placeholder="/Users/.../README.md 或 https://raw.githubusercontent.com/..." />
                    </Form.Item>
                    <Form.Item name="description" label="分类描述 (选填)">
                        <Input placeholder="说明该订阅源的用途或镜头风格" />
                    </Form.Item>
                </Form>
            </Modal>
        </div>
    );
}
