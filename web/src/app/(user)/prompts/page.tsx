"use client";

import { FolderPlus, Plus, RefreshCw, Search } from "lucide-react";
import { type UIEvent, useEffect, useState } from "react";
import { App, Button, Empty, Form, Input, Modal, Select, Spin, Tag } from "antd";

import { PromptCard } from "@/components/prompts/prompt-card";
import { PromptDetailDialog } from "@/components/prompts/prompt-detail-dialog";
import { usePromptList } from "@/components/prompts/use-prompt-list";
import { useCopyText } from "@/hooks/use-copy-text";
import { cn } from "@/lib/utils";
import { useAssetStore } from "@/stores/use-asset-store";
import { ALL_PROMPTS_OPTION, savePromptCategory, syncPrompts, type Prompt } from "@/services/api/prompts";

export default function PromptsPage() {
    const { message } = App.useApp();
    const [titleInput, setTitleInput] = useState("");
    const [titleKeyword, setTitleKeyword] = useState("");
    const [selectedTags, setSelectedTags] = useState<string[]>([]);
    const [selectedCategory, setSelectedCategory] = useState(ALL_PROMPTS_OPTION);
    const [selectedPrompt, setSelectedPrompt] = useState<Prompt | null>(null);
    const [syncing, setSyncing] = useState(false);
    const [addModalOpen, setAddModalOpen] = useState(false);
    const [addForm] = Form.useForm();
    const addAsset = useAssetStore((state) => state.addAsset);
    const copyText = useCopyText();
    const { query, items: promptItems, tags: promptTags, categories: promptCategoryOptions, total: totalPrompts } = usePromptList({ keyword: titleKeyword, tags: selectedTags, category: selectedCategory });

    const handleSync = async () => {
        try {
            setSyncing(true);
            await syncPrompts();
            message.success("提示词全量调度与同步完成！");
            void query.refetch();
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
            message.success("订阅源已添加，正在立即触发拉取...");
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

    const savePromptAsset = (item: Prompt) => {
        addAsset({ kind: "text", title: item.title, coverUrl: item.coverUrl, tags: item.tags, source: item.category, data: { content: item.prompt }, metadata: { source: "prompt-library", promptId: item.id, githubUrl: item.githubUrl } });
        message.success("已加入我的素材");
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
                        <p className="mt-3 text-sm text-stone-500 dark:text-stone-400">共 {totalPrompts} 条提示词，按标题、标签与分类快速查找灵感。</p>
                        <div className="mt-5 flex items-center justify-center gap-3">
                            <Button
                                type="primary"
                                icon={<RefreshCw className={cn("size-4", syncing && "animate-spin")} />}
                                loading={syncing}
                                onClick={handleSync}
                            >
                                即时调度与全量同步
                            </Button>
                            <Button
                                icon={<Plus className="size-4" />}
                                onClick={() => setAddModalOpen(true)}
                            >
                                添加订阅源 (URL / 本地路径)
                            </Button>
                        </div>
                    </div>
                    {query.isLoading ? (
                        <div className="flex h-60 items-center justify-center">
                            <Spin />
                        </div>
                    ) : null}
                    {!query.isLoading ? (
                        <>
                            <div className="mx-auto mt-8 w-full max-w-2xl">
                                <Input size="large" className="w-full" prefix={<Search className="size-4 text-stone-400" />} value={titleInput} placeholder="按标题查询，按 Enter 搜索" onChange={(event) => setTitleInput(event.target.value)} onPressEnter={searchByTitleInput} />
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
                                    <div className="flex flex-wrap gap-2">
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
                                    onCopy={() => copyText(item.prompt, "提示词已复制")}
                                    extraAction={
                                        <Button size="small" icon={<FolderPlus className="size-3.5" />} onClick={() => savePromptAsset(item)}>
                                            加入我的素材
                                        </Button>
                                    }
                                />
                            ))}
                        </div>
                        {promptItems.length === 0 ? <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有找到匹配的提示词" className="py-16" /> : null}
                        <div className="mx-auto mt-6 max-w-7xl text-center text-xs text-stone-500 dark:text-stone-400">
                            {query.isFetchingNextPage ? "加载中..." : query.hasNextPage ? "继续向下滚动加载更多" : promptItems.length > 0 ? "已经到底了" : null}
                        </div>
                    </div>
                ) : null}
            </main>

            <PromptDetailDialog prompt={selectedPrompt} onClose={() => setSelectedPrompt(null)} onCopy={(prompt) => copyText(prompt, "提示词已复制")} onSaveAsset={savePromptAsset} />

            <Modal
                title="添加提示词订阅源"
                open={addModalOpen}
                onCancel={() => setAddModalOpen(false)}
                onOk={() => addForm.submit()}
                okText="保存并拉取"
                cancelText="取消"
            >
                <Form form={addForm} layout="vertical" onFinish={handleAddSource} initialValues={{ sourceType: "local_markdown" }}>
                    <Form.Item name="category" label="分类唯一标识 (英文/拼音)" rules={[{ required: true, message: "请输入分类标识，如 custom-prompts" }]}>
                        <Input placeholder="custom-prompts" />
                    </Form.Item>
                    <Form.Item name="name" label="分类显示名称" rules={[{ required: true, message: "请输入显示名称" }]}>
                        <Input placeholder="我的专属分镜提示词" />
                    </Form.Item>
                    <Form.Item name="sourceType" label="源类型" rules={[{ required: true }]}>
                        <Select options={[
                            { value: "local_markdown", label: "本地 Markdown 文件 (绝对路径)" },
                            { value: "custom_url", label: "远端 URL (GitHub Raw / Markdown / JSON)" },
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
