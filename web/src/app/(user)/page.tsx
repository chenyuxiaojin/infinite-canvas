"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { App, Badge, Button, Card, Empty, Spin, Tag } from "antd";
import {
    Activity,
    ArrowRight,
    Clapperboard,
    Cpu,
    ExternalLink,
    FileText,
    Film,
    FolderKanban,
    HardDrive,
    Layers,
    Plus,
    RefreshCw,
    Sparkles,
    Video,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { fetchPrompts, type Prompt } from "@/services/api/prompts";
import { isDesktopRuntime, probeDesktopRuntime, type DesktopRuntimeReport } from "@/services/desktop-runtime";
import { useCanvasStore } from "./canvas/stores/use-canvas-store";

export default function IndexPage() {
    const { message } = App.useApp();
    const router = useRouter();
    const hydrated = useCanvasStore((state) => state.hydrated);
    const projects = useCanvasStore((state) => state.projects);
    const createProject = useCanvasStore((state) => state.createProject);

    const [runtimeReport, setRuntimeReport] = useState<DesktopRuntimeReport | null>(null);
    const [reportLoading, setReportLoading] = useState(false);
    const [featuredPrompts, setFeaturedPrompts] = useState<Prompt[]>([]);

    useEffect(() => {
        if (isDesktopRuntime()) {
            setReportLoading(true);
            probeDesktopRuntime()
                .then(setRuntimeReport)
                .catch(() => {})
                .finally(() => setReportLoading(false));
        }

        fetchPrompts({ pageSize: 6 })
            .then((data) => setFeaturedPrompts(data.items || []))
            .catch(() => {});
    }, []);

    const handleCreateProject = () => {
        if (!hydrated) {
            message.info("画布存储正在加载，请稍候...");
            return;
        }
        const index = projects.length + 1;
        const newId = createProject(`案例分镜工程 EP0${index}`);
        router.push(`/canvas/${newId}`);
    };

    const davinciConnector = runtimeReport?.connectors?.find((c) => c.provider === "davinci_resolve");
    const eagleConnector = runtimeReport?.connectors?.find((c) => c.provider === "eagle");
    const ffmpegStatus = runtimeReport?.ffmpeg?.status === "available";

    return (
        <main className="h-full overflow-y-auto bg-stone-950 text-stone-100 px-6 py-8">
            <div className="mx-auto max-w-7xl space-y-8">
                {/* 顶部标题区 */}
                <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between border-b border-stone-800/80 pb-6">
                    <div>
                        <div className="flex items-center gap-2.5">
                            <span className="flex size-9 items-center justify-center rounded-lg bg-emerald-500/10 text-emerald-400 ring-1 ring-emerald-500/30">
                                <Clapperboard className="size-5" />
                            </span>
                            <h1 className="text-2xl font-bold tracking-tight text-white">AI 编导 · 导演工程台</h1>
                            <Tag color="cyan" className="m-0 text-xs">macOS 原生桌面版</Tag>
                        </div>
                        <p className="mt-1.5 text-sm text-stone-400">
                            以分镜首帧定死为核心的 AI 影视编导工作流 · 本地媒体零泄露 · 达芬奇与外部 Agent 实时联通
                        </p>
                    </div>
                    <div className="flex items-center gap-3">
                        <Button
                            type="primary"
                            size="large"
                            icon={<Plus className="size-4" />}
                            className="bg-emerald-600 hover:bg-emerald-500 border-emerald-500 font-medium"
                            onClick={handleCreateProject}
                        >
                            新建片子工程
                        </Button>
                        <Button
                            size="large"
                            icon={<FolderKanban className="size-4" />}
                            onClick={() => router.push("/canvas")}
                            className="border-stone-700 bg-stone-900 text-stone-200 hover:text-white"
                        >
                            全部画布矩阵
                        </Button>
                    </div>
                </div>

                {/* 软硬件与外设连通状态条 */}
                <div className="rounded-xl border border-stone-800 bg-stone-900/60 p-4 shadow-sm backdrop-blur">
                    <div className="flex flex-wrap items-center justify-between gap-4">
                        <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-stone-400">
                            <Cpu className="size-4 text-emerald-400" />
                            <span>本地工作台运行时状态</span>
                        </div>
                        <div className="flex flex-wrap items-center gap-4 text-xs">
                            <div className="flex items-center gap-2 rounded-md bg-stone-950 px-3 py-1.5 ring-1 ring-stone-800">
                                <span className="size-2 rounded-full bg-emerald-400 animate-pulse" />
                                <span className="text-stone-300">桌面运行时: 3100/3101</span>
                            </div>

                            <div className="flex items-center gap-2 rounded-md bg-stone-950 px-3 py-1.5 ring-1 ring-stone-800">
                                <span className="size-2 rounded-full bg-cyan-400" />
                                <span className="text-stone-300">Agent Bridge: 127.0.0.1:3102</span>
                            </div>

                            <div className="flex items-center gap-2 rounded-md bg-stone-950 px-3 py-1.5 ring-1 ring-stone-800">
                                <span className={cn("size-2 rounded-full", ffmpegStatus ? "bg-emerald-400" : "bg-amber-400")} />
                                <span className="text-stone-300">FFmpeg: {ffmpegStatus ? "已就绪" : "待探测"}</span>
                            </div>

                            <div className="flex items-center gap-2 rounded-md bg-stone-950 px-3 py-1.5 ring-1 ring-stone-800">
                                <span className={cn("size-2 rounded-full", davinciConnector?.status === "ready" ? "bg-emerald-400" : "bg-stone-500")} />
                                <span className="text-stone-300">
                                    DaVinci Resolve: {davinciConnector?.status === "ready" ? "已连接" : "未启动"}
                                </span>
                            </div>

                            <div className="flex items-center gap-2 rounded-md bg-stone-950 px-3 py-1.5 ring-1 ring-stone-800">
                                <span className={cn("size-2 rounded-full", eagleConnector?.status === "ready" ? "bg-emerald-400" : "bg-stone-500")} />
                                <span className="text-stone-300">
                                    Eagle 素材库: {eagleConnector?.status === "ready" ? "已联通" : "未运行"}
                                </span>
                            </div>
                        </div>
                    </div>
                </div>

                {/* 活跃片子工程列表 */}
                <section>
                    <div className="mb-4 flex items-center justify-between">
                        <div className="flex items-center gap-2">
                            <Layers className="size-4 text-emerald-400" />
                            <h2 className="text-base font-semibold text-white">活跃片子案例工程</h2>
                            <span className="text-xs text-stone-500">（共 {projects.length} 个本地工程）</span>
                        </div>
                        <Button
                            type="link"
                            size="small"
                            onClick={() => router.push("/canvas")}
                            className="text-stone-400 hover:text-emerald-400 text-xs p-0 flex items-center gap-1"
                        >
                            查看全部 <ArrowRight className="size-3" />
                        </Button>
                    </div>

                    {projects.length === 0 ? (
                        <div className="rounded-xl border border-dashed border-stone-800 py-12 text-center">
                            <Film className="mx-auto size-10 text-stone-600 mb-3" />
                            <h3 className="text-sm font-medium text-stone-300">暂无片子工程</h3>
                            <p className="text-xs text-stone-500 mt-1 mb-4">创建你的第一个分镜工程，开始首帧定死与视频生成编排</p>
                            <Button type="primary" onClick={handleCreateProject} className="bg-emerald-600 hover:bg-emerald-500">
                                立即创建
                            </Button>
                        </div>
                    ) : (
                        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
                            {projects.slice(0, 8).map((proj) => {
                                const nodeCount = proj.nodes?.length || 0;
                                const videoCount = proj.nodes?.filter((n) => n.type === "video").length || 0;
                                const imageCount = proj.nodes?.filter((n) => n.type === "image").length || 0;

                                return (
                                    <div
                                        key={proj.id}
                                        onClick={() => router.push(`/canvas/${proj.id}`)}
                                        className="group relative cursor-pointer rounded-xl border border-stone-800/80 bg-stone-900/50 p-5 transition-all hover:border-emerald-500/50 hover:bg-stone-900 hover:shadow-lg hover:shadow-emerald-950/20"
                                    >
                                        <div className="flex items-start justify-between">
                                            <span className="flex size-9 items-center justify-center rounded-lg bg-stone-800 text-emerald-400 group-hover:bg-emerald-500/20">
                                                <Film className="size-4" />
                                            </span>
                                            <span className="text-[11px] text-stone-500">
                                                {new Date(proj.updatedAt || Date.now()).toLocaleDateString("zh-CN")}
                                            </span>
                                        </div>
                                        <h3 className="mt-3 truncate text-sm font-semibold text-white group-hover:text-emerald-400">
                                            {proj.title || "未命名工程"}
                                        </h3>
                                        <div className="mt-3 flex items-center gap-3 text-xs text-stone-400">
                                            <span>{nodeCount} 节点</span>
                                            <span>·</span>
                                            <span>{imageCount} 张图片</span>
                                            <span>·</span>
                                            <span>{videoCount} 条视频</span>
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    )}
                </section>

                {/* 灵感与提示词快速通道 */}
                <section className="border-t border-stone-800/80 pt-6">
                    <div className="mb-4 flex items-center justify-between">
                        <div className="flex items-center gap-2">
                            <Sparkles className="size-4 text-emerald-400" />
                            <h2 className="text-base font-semibold text-white">影视编导与运镜提示词库</h2>
                            <Tag color="default" className="bg-stone-900 text-stone-400 border-stone-800 text-xs">
                                已接入本地参考仓库与远端 URL
                            </Tag>
                        </div>
                        <Button
                            type="link"
                            size="small"
                            onClick={() => router.push("/prompts")}
                            className="text-stone-400 hover:text-emerald-400 text-xs p-0 flex items-center gap-1"
                        >
                            打开提示词中心 <ArrowRight className="size-3" />
                        </Button>
                    </div>

                    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                        {featuredPrompts.map((item) => (
                            <div
                                key={item.id}
                                onClick={() => router.push("/prompts")}
                                className="cursor-pointer rounded-lg border border-stone-800/70 bg-stone-900/30 p-3.5 transition hover:border-stone-700 hover:bg-stone-900/60"
                            >
                                <div className="flex items-center justify-between gap-2">
                                    <h4 className="truncate text-xs font-medium text-stone-200">{item.title}</h4>
                                    <Tag className="m-0 border-0 bg-stone-800 text-[10px] text-stone-400">
                                        {item.category}
                                    </Tag>
                                </div>
                                <p className="mt-2 line-clamp-2 text-xs text-stone-400 leading-relaxed font-mono">
                                    {item.prompt}
                                </p>
                            </div>
                        ))}
                    </div>
                </section>
            </div>
        </main>
    );
}

