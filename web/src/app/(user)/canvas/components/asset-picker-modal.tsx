"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { App, Button, Empty, Input, Modal, Pagination, Spin, Tag } from "antd";
import { ImagePlus, Plus, Search } from "lucide-react";

import { useStoredMediaSource } from "@/hooks/use-stored-media-source";
import { assetMediaReference } from "@/services/asset-media-reference";
import { cn } from "@/lib/utils";
import { useAssetStore, type Asset } from "@/stores/use-asset-store";
import { uploadAssetMediaFile } from "@/services/file-storage";
import { uploadImage } from "@/services/image-storage";
import type { InsertAssetPayload } from "../types";

export type { InsertAssetPayload } from "../types";

type Props = {
    open: boolean;
    onInsert: (payload: InsertAssetPayload) => void;
    onClose: () => void;
};

export function AssetPickerModal({ open, onInsert, onClose }: Props) {
    return (
        <Modal title="我的素材" open={open} onCancel={onClose} footer={null} width={860} destroyOnHidden styles={{ body: { padding: "0 24px 24px", minHeight: 480 } }}>
            <MyAssetsTab onInsert={onInsert} />
        </Modal>
    );
}

const PAGE_SIZE = 8;

const kindOptions = [
    { label: "全部", value: "all" },
    { label: "文本", value: "text" },
    { label: "图片", value: "image" },
    { label: "视频", value: "video" },
    { label: "音频", value: "audio" },
];

function PickerCard({ asset, loading, onClick }: { asset: Asset; loading?: boolean; onClick: () => void }) {
    const { title, kind } = asset;
    const reference = assetMediaReference(asset, true);
    const source = useStoredMediaSource({ ...reference, observe: true });
    const cover = reference.image ? source.src : "";
    return (
        <button
            ref={source.ref}
            type="button"
            className="group relative cursor-pointer overflow-hidden rounded-lg border border-stone-200 bg-white text-left transition hover:border-stone-400 hover:shadow-md dark:border-stone-700 dark:bg-stone-900 dark:hover:border-stone-500"
            onClick={onClick}
            disabled={loading}
        >
            {cover ? (
                <img src={cover} alt={title} className="aspect-[4/3] w-full object-cover" />
            ) : (
                <div className="flex aspect-[4/3] items-center justify-center bg-stone-100 p-3 text-center text-xs leading-5 text-stone-500 dark:bg-stone-800 dark:text-stone-400">{title}</div>
            )}
            <div className="p-2.5">
                <div className="flex items-center justify-between gap-2">
                    <span className="line-clamp-1 text-xs font-medium text-stone-800 dark:text-stone-200">{title}</span>
                    <Tag className="m-0 shrink-0 text-[10px]">{kind === "image" ? "图片" : kind === "video" ? "视频" : kind === "audio" ? "音频" : "文本"}</Tag>
                </div>
            </div>
            {loading && (
                <div className="absolute inset-0 flex items-center justify-center bg-white/60 dark:bg-stone-900/60">
                    <Spin size="small" />
                </div>
            )}
            <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-stone-950/0 text-sm font-medium text-white opacity-0 transition group-hover:bg-stone-950/55 group-hover:opacity-100">插入</div>
        </button>
    );
}

function MyAssetsTab({ onInsert }: { onInsert: (payload: InsertAssetPayload) => void }) {
    const { message } = App.useApp();
    const assets = useAssetStore((state) => state.assets);
    const addAsset = useAssetStore((state) => state.addAsset);
    const [keyword, setKeyword] = useState("");
    const [kindFilter, setKindFilter] = useState("all");
    const [page, setPage] = useState(1);
    const [createOpen, setCreateOpen] = useState(false);
    const [createKind, setCreateKind] = useState<"text" | "image" | "video" | "audio">("image");
    const [createTitle, setCreateTitle] = useState("");
    const [createText, setCreateText] = useState("");
    const [createUrl, setCreateUrl] = useState("");
    const [saving, setSaving] = useState(false);
    const fileInputRef = useRef<HTMLInputElement>(null);
    const [selectedFile, setSelectedFile] = useState<File | null>(null);

    const filtered = useMemo(() => {
        const query = keyword.trim().toLowerCase();
        return assets
            .filter((a) => a.kind === "text" || a.kind === "image" || a.kind === "video" || a.kind === "audio")
            .filter((a) => kindFilter === "all" || a.kind === kindFilter)
            .filter((a) => !query || [a.title, ...(a.tags || [])].join(" ").toLowerCase().includes(query));
    }, [assets, keyword, kindFilter]);

    const visible = useMemo(() => filtered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE), [filtered, page]);

    useEffect(() => {
        const maxPage = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
        setPage((v) => Math.min(v, maxPage));
    }, [filtered.length]);

    const handleInsert = (asset: Asset) => {
        if (asset.kind === "text") {
            onInsert({ kind: "text", content: asset.data.content, title: asset.title, assetId: asset.id, source: "asset" });
        } else {
            onInsert(
                asset.kind === "video"
                    ? { kind: "video", url: asset.data.url, storageKey: asset.data.storageKey, title: asset.title, assetId: asset.id, width: asset.data.width, height: asset.data.height, bytes: asset.data.bytes, mimeType: asset.data.mimeType, source: "asset" }
                    : asset.kind === "audio"
                      ? { kind: "audio", url: asset.data.url, storageKey: asset.data.storageKey, title: asset.title, assetId: asset.id, bytes: asset.data.bytes, mimeType: asset.data.mimeType, durationMs: asset.data.durationMs, source: "asset" }
                      : { kind: "image", dataUrl: asset.data.dataUrl, storageKey: asset.data.storageKey, title: asset.title, assetId: asset.id, width: asset.data.width, height: asset.data.height, bytes: asset.data.bytes, mimeType: asset.data.mimeType, source: "asset" },
            );
        }
    };

    const resetCreateForm = () => {
        setCreateTitle("");
        setCreateText("");
        setCreateUrl("");
        setSelectedFile(null);
        if (fileInputRef.current) fileInputRef.current.value = "";
    };

    const createAsset = async () => {
        const title = createTitle.trim();
        if (!title) {
            message.error("请输入素材名称");
            return;
        }
        setSaving(true);
        try {
            if (createKind === "text") {
                const content = createText.trim();
                if (!content) {
                    message.error("请输入文本内容");
                    return;
                }
                addAsset({ kind: "text", title, coverUrl: "", tags: [], source: "素材选择器", data: { content } });
            } else if (createKind === "image") {
                if (!selectedFile && !createUrl.trim()) {
                    message.error("请选择图片或填写图片 URL");
                    return;
                }
                const stored = selectedFile ? await uploadImage(selectedFile, { retainDisplayUrl: false }) : null;
                addAsset({
                    kind: "image",
                    title,
                    coverUrl: stored?.url || createUrl.trim(),
                    tags: [],
                    source: "素材选择器",
                    data: stored ? { dataUrl: stored.url, storageKey: stored.storageKey, width: stored.width, height: stored.height, bytes: stored.bytes, mimeType: stored.mimeType } : { dataUrl: createUrl.trim(), width: 0, height: 0, bytes: 0, mimeType: "image/*" },
                });
            } else if (createKind === "video") {
                if (!selectedFile && !createUrl.trim()) {
                    message.error("请选择视频或填写视频 URL");
                    return;
                }
                const stored = selectedFile ? await uploadAssetMediaFile(selectedFile, "asset-video", false) : null;
                addAsset({
                    kind: "video",
                    title,
                    coverUrl: "",
                    tags: [],
                    source: "素材选择器",
                    data: stored
                        ? { url: stored.url, storageKey: stored.storageKey, width: stored.width || 0, height: stored.height || 0, bytes: stored.bytes, mimeType: stored.mimeType }
                        : { url: createUrl.trim(), width: 0, height: 0, bytes: 0, mimeType: "video/mp4" },
                });
            } else {
                if (!selectedFile && !createUrl.trim()) {
                    message.error("请选择音频或填写音频 URL");
                    return;
                }
                const stored = selectedFile ? await uploadAssetMediaFile(selectedFile, "asset-audio", false) : null;
                addAsset({
                    kind: "audio",
                    title,
                    coverUrl: "",
                    tags: [],
                    source: "素材选择器",
                    data: stored ? { url: stored.url, storageKey: stored.storageKey, bytes: stored.bytes, mimeType: stored.mimeType, durationMs: stored.durationMs } : { url: createUrl.trim(), mimeType: "audio/mpeg" },
                });
            }
            message.success("素材已新增");
            setCreateOpen(false);
            resetCreateForm();
        } catch (error) {
            message.error(error instanceof Error ? error.message : "新增素材失败");
        } finally {
            setSaving(false);
        }
    };

    return (
        <div className="space-y-4">
            <div className="flex flex-wrap items-center gap-3">
                <Input
                    className="w-56"
                    size="small"
                    prefix={<Search className="size-3.5 text-stone-400" />}
                    placeholder="搜索素材"
                    value={keyword}
                    allowClear
                    onChange={(e) => {
                        setPage(1);
                        setKeyword(e.target.value);
                    }}
                />
                <div className="flex gap-1.5">
                    {kindOptions.map((opt) => (
                        <Tag.CheckableTag
                            key={opt.value}
                            checked={kindFilter === opt.value}
                            className={cn("prompt-filter-tag", kindFilter === opt.value && "is-active")}
                            onChange={() => {
                                setPage(1);
                                setKindFilter(opt.value);
                            }}
                        >
                            {opt.label}
                        </Tag.CheckableTag>
                    ))}
                </div>
                <Button size="small" icon={<Plus className="size-3.5" />} onClick={() => setCreateOpen(true)}>
                    新增素材
                </Button>
            </div>

            {visible.length ? (
                <div className="grid grid-cols-4 gap-3">
                    {visible.map((asset) => (
                        <PickerCard key={asset.id} asset={asset} onClick={() => handleInsert(asset)} />
                    ))}
                </div>
            ) : (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有素材" className="py-12" />
            )}

            {filtered.length > PAGE_SIZE && (
                <div className="flex justify-center">
                    <Pagination size="small" current={page} pageSize={PAGE_SIZE} total={filtered.length} onChange={setPage} showSizeChanger={false} />
                </div>
            )}
            <Modal
                title="新增素材"
                open={createOpen}
                onCancel={() => {
                    setCreateOpen(false);
                    resetCreateForm();
                }}
                onOk={() => void createAsset()}
                okText="保存"
                confirmLoading={saving}
                destroyOnHidden
            >
                <div className="space-y-3 pt-2">
                    <div className="flex gap-2">
                        {[
                            { value: "image" as const, label: "图片" },
                            { value: "text" as const, label: "文本" },
                            { value: "video" as const, label: "视频" },
                            { value: "audio" as const, label: "音频" },
                        ].map((item) => (
                            <Tag.CheckableTag key={item.value} checked={createKind === item.value} className={cn("prompt-filter-tag", createKind === item.value && "is-active")} onChange={() => setCreateKind(item.value)}>
                                {item.label}
                            </Tag.CheckableTag>
                        ))}
                    </div>
                    <Input value={createTitle} placeholder="素材名称" onChange={(event) => setCreateTitle(event.target.value)} />
                    {createKind === "text" ? (
                        <Input.TextArea value={createText} autoSize={{ minRows: 5, maxRows: 10 }} placeholder="文本内容" onChange={(event) => setCreateText(event.target.value)} />
                    ) : (
                        <div className="space-y-2">
                            <input ref={fileInputRef} type="file" accept={createKind === "image" ? "image/*" : createKind === "video" ? "video/*" : "audio/mpeg,audio/wav,audio/x-wav,.mp3,.wav"} className="hidden" onChange={(event) => setSelectedFile(event.target.files?.[0] || null)} />
                            <Button icon={<ImagePlus className="size-4" />} onClick={() => fileInputRef.current?.click()}>
                                {selectedFile ? selectedFile.name : createKind === "image" ? "选择图片" : createKind === "video" ? "选择视频" : "选择音频"}
                            </Button>
                            <Input value={createUrl} placeholder={createKind === "image" ? "图片 URL" : createKind === "video" ? "视频 URL" : "音频 URL"} onChange={(event) => setCreateUrl(event.target.value)} />
                        </div>
                    )}
                </div>
            </Modal>
        </div>
    );
}
