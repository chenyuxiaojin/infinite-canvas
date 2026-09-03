"use client";

import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useRouter } from "next/navigation";
import { App, Button, Spin } from "antd";
import {
    ArrowRight,
    Bot,
    CheckCircle2,
    ChevronRight,
    Clapperboard,
    Film,
    FolderOpen,
    LayoutGrid,
    Plus,
    Terminal,
} from "lucide-react";

import { bindCanvasProjectDirectory, selectFilmDirectory } from "@/services/desktop-terminal";
import { isDesktopRuntime, probeDesktopRuntime, type DesktopRuntimeReport } from "@/services/desktop-runtime";
import { useCanvasStore } from "./canvas/stores/use-canvas-store";

export default function IndexPage() {
    const { message } = App.useApp();
    const router = useRouter();
    const hydrated = useCanvasStore((state) => state.hydrated);
    const projects = useCanvasStore((state) => state.projects);
    const createProject = useCanvasStore((state) => state.createProject);
    const refreshFromDesktop = useCanvasStore((state) => state.refreshFromDesktop);
    const [runtimeReport, setRuntimeReport] = useState<DesktopRuntimeReport | null>(null);
    const [creating, setCreating] = useState(false);
    const [desktopSyncStatus, setDesktopSyncStatus] = useState<"idle" | "syncing" | "synced" | "failed">("idle");
    const [desktopSyncError, setDesktopSyncError] = useState("");

    useEffect(() => {
        if (!isDesktopRuntime()) return;
        probeDesktopRuntime().then(setRuntimeReport).catch(() => undefined);
    }, []);

    useEffect(() => {
        if (!hydrated || !isDesktopRuntime()) return;
        setDesktopSyncStatus("syncing");
        void refreshFromDesktop()
            .then(() => setDesktopSyncStatus("synced"))
            .catch((error) => {
                setDesktopSyncError(error instanceof Error ? error.message : String(error));
                setDesktopSyncStatus("failed");
            });
    }, [hydrated, refreshFromDesktop]);

    const orderedProjects = useMemo(
        () => [...projects].sort((left, right) => Date.parse(right.updatedAt || "") - Date.parse(left.updatedAt || "")),
        [projects],
    );
    const latestProject = orderedProjects.find((project) => /^案例\d/.test(project.title || "") && (project.nodes?.length || 0) > 0) || orderedProjects[0];

    const openLatest = (panel?: "agent" | "terminal") => {
        if (!latestProject) {
            void handleCreateProject();
            return;
        }
        router.push(`/canvas/${latestProject.id}${panel ? `?panel=${panel}` : ""}`);
    };

    const handleCreateProject = async () => {
        if (!hydrated || creating) {
            if (!hydrated) message.info("正在读取本地画布，请稍候");
            return;
        }
        setCreating(true);
        try {
            let directory: string | null = null;
            if (isDesktopRuntime()) {
                directory = await selectFilmDirectory();
                if (!directory) return;
            }
            const folderName = directory?.split("/").filter(Boolean).at(-1);
            const title = folderName || `新片子 ${projects.length + 1}`;
            const newId = createProject(title);
            if (directory) {
                try {
                    await bindCanvasProjectDirectory(newId, title, directory);
                } catch (error) {
                    message.warning(error instanceof Error ? error.message : "画布已创建，但片子目录连接失败");
                }
            }
            router.push(`/canvas/${newId}`);
        } finally {
            setCreating(false);
        }
    };

    const ffmpegReady = runtimeReport?.ffmpeg?.status === "available";
    const connectedTools = runtimeReport?.connectors?.filter((connector) => connector.status === "ready").length || 0;

    return (
        <main className="h-full overflow-y-auto bg-stone-50 text-stone-950 dark:bg-stone-950 dark:text-stone-100">
            <div className="mx-auto max-w-7xl px-6 py-8 lg:py-10">
                <section className="overflow-hidden rounded-3xl border border-stone-200 bg-white shadow-sm dark:border-stone-800 dark:bg-stone-900">
                    <div className="grid gap-8 p-7 lg:grid-cols-[1.35fr_.65fr] lg:p-10">
                        <div className="flex min-w-0 flex-col justify-center">
                            <div className="mb-4 inline-flex w-fit items-center gap-2 rounded-full bg-stone-100 px-3 py-1.5 text-xs font-medium text-stone-600 dark:bg-stone-800 dark:text-stone-300">
                                <Clapperboard className="size-3.5" />
                                一部片子，一张画布，一个工作目录
                            </div>
                            <h1 className="max-w-2xl text-3xl font-semibold tracking-tight sm:text-4xl">
                                {latestProject ? "继续把这部片子做完" : "从一部片子开始创作"}
                            </h1>
                            <p className="mt-3 max-w-xl text-sm leading-6 text-stone-500 dark:text-stone-400">
                                进入画布后，选中图片、文字或视频，右侧 AI 会自动带上这些节点。也可以直接在同一个侧栏启动 Codex 或 Claude。
                            </p>

                            {latestProject ? (
                                <div className="mt-7 rounded-2xl border border-stone-200 bg-stone-50 p-5 dark:border-stone-700 dark:bg-stone-950/50">
                                    <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                                        <div className="min-w-0">
                                            <div className="text-xs text-stone-500 dark:text-stone-400">最近编辑</div>
                                            <div className="mt-1 truncate text-lg font-semibold">{latestProject.title || "未命名片子"}</div>
                                            <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-stone-500 dark:text-stone-400">
                                                <span>{latestProject.nodes?.length || 0} 个节点</span>
                                                <span>{latestProject.connections?.length || 0} 条连线</span>
                                                <span>{new Date(latestProject.updatedAt).toLocaleString("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" })}</span>
                                            </div>
                                        </div>
                                        <Button type="primary" size="large" onClick={() => openLatest()} className="!h-11 !rounded-xl !px-5">
                                            继续创作 <ArrowRight className="ml-1 size-4" />
                                        </Button>
                                    </div>
                                </div>
                            ) : (
                                <Button type="primary" size="large" loading={creating} onClick={() => void handleCreateProject()} className="mt-7 !h-12 w-fit !rounded-xl !px-6">
                                    选择片子目录并新建画布
                                </Button>
                            )}
                        </div>

                        <div className="grid grid-cols-2 gap-3 self-stretch">
                            <HomeAction icon={<Plus />} title="新建片子" description="选目录，建画布" onClick={() => void handleCreateProject()} loading={creating} />
                            <HomeAction icon={<LayoutGrid />} title="全部画布" description={`${projects.length} 个本地项目`} onClick={() => router.push("/canvas")} />
                            <HomeAction icon={<Bot />} title="画布 Agent" description="对话并操作节点" onClick={() => openLatest("agent")} />
                            <HomeAction icon={<Terminal />} title="本地 AI" description="启动 Codex / Claude" onClick={() => openLatest("terminal")} />
                        </div>
                    </div>
                </section>

                <section className="mt-9">
                    <div className="mb-4 flex items-center justify-between">
                        <div>
                            <h2 className="text-lg font-semibold">你的片子</h2>
                            <p className="mt-1 text-sm text-stone-500 dark:text-stone-400">点开就回到上次的画布位置和工作状态</p>
                        </div>
                        {projects.length > 6 ? (
                            <Button type="text" onClick={() => router.push("/canvas")}>
                                查看全部 <ChevronRight className="size-4" />
                            </Button>
                        ) : null}
                    </div>

                    {!hydrated ? (
                        <div className="grid h-40 place-items-center rounded-2xl border border-stone-200 bg-white dark:border-stone-800 dark:bg-stone-900"><Spin /></div>
                    ) : orderedProjects.length ? (
                        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
                            {orderedProjects.slice(0, 6).map((project) => (
                                <button
                                    key={project.id}
                                    type="button"
                                    onClick={() => router.push(`/canvas/${project.id}`)}
                                    className="group flex min-h-36 cursor-pointer flex-col rounded-2xl border border-stone-200 bg-white p-5 text-left transition hover:-translate-y-0.5 hover:border-stone-400 hover:shadow-md dark:border-stone-800 dark:bg-stone-900 dark:hover:border-stone-600"
                                >
                                    <div className="flex items-start justify-between gap-3">
                                        <span className="grid size-10 place-items-center rounded-xl bg-stone-100 text-stone-700 dark:bg-stone-800 dark:text-stone-200"><Film className="size-5" /></span>
                                        <span className="text-xs text-stone-400">{new Date(project.updatedAt).toLocaleDateString("zh-CN")}</span>
                                    </div>
                                    <div className="mt-4 truncate font-medium">{project.title || "未命名片子"}</div>
                                    <div className="mt-auto flex items-center justify-between pt-3 text-xs text-stone-500 dark:text-stone-400">
                                        <span>{project.nodes?.length || 0} 节点 · {project.connections?.length || 0} 连线</span>
                                        <ArrowRight className="size-4 opacity-0 transition group-hover:translate-x-0.5 group-hover:opacity-100" />
                                    </div>
                                </button>
                            ))}
                        </div>
                    ) : (
                        <button type="button" onClick={() => void handleCreateProject()} className="flex h-40 w-full cursor-pointer flex-col items-center justify-center rounded-2xl border border-dashed border-stone-300 bg-white text-stone-500 transition hover:border-stone-500 hover:text-stone-800 dark:border-stone-700 dark:bg-stone-900 dark:hover:text-stone-200">
                            <FolderOpen className="size-7" />
                            <span className="mt-3 text-sm font-medium">选择一个片子目录</span>
                        </button>
                    )}
                </section>

                <section className="mt-9 grid gap-4 border-t border-stone-200 pt-7 md:grid-cols-[1fr_auto] md:items-center dark:border-stone-800">
                    <div className="flex items-start gap-3">
                        <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-full bg-emerald-100 text-emerald-700 dark:bg-emerald-950 dark:text-emerald-300"><CheckCircle2 className="size-4" /></span>
                        <div>
                            <div className="text-sm font-medium">选中节点后，右侧两个 AI 入口会共享这些内容</div>
                            <div className="mt-1 text-xs leading-5 text-stone-500 dark:text-stone-400">画布 Agent 适合直接生成和修改节点；本地 AI 适合调用片子目录里的文件、脚本和完整工作流。</div>
                        </div>
                    </div>
                    <div className="flex flex-wrap gap-2 text-xs text-stone-500 dark:text-stone-400">
                        <span className="rounded-full bg-stone-100 px-3 py-1.5 dark:bg-stone-900">桌面服务 {runtimeReport ? "已连接" : "检测中"}</span>
                        <span title={desktopSyncError} className="rounded-full bg-stone-100 px-3 py-1.5 dark:bg-stone-900">
                            画布数据 {desktopSyncStatus === "synced" ? "已同步" : desktopSyncStatus === "failed" ? "读取失败" : "同步中"}
                        </span>
                        <span className="rounded-full bg-stone-100 px-3 py-1.5 dark:bg-stone-900">本地视频 {ffmpegReady ? "可用" : "待检测"}</span>
                        <span className="rounded-full bg-stone-100 px-3 py-1.5 dark:bg-stone-900">外部工具 {connectedTools} 个在线</span>
                    </div>
                </section>
            </div>
        </main>
    );
}

function HomeAction({ icon, title, description, onClick, loading = false }: { icon: ReactNode; title: string; description: string; onClick: () => void; loading?: boolean }) {
    return (
        <button
            type="button"
            onClick={onClick}
            disabled={loading}
            className="group flex min-h-32 cursor-pointer flex-col items-start rounded-2xl border border-stone-200 bg-stone-50 p-4 text-left transition hover:border-stone-400 hover:bg-stone-100 disabled:cursor-wait disabled:opacity-60 dark:border-stone-700 dark:bg-stone-800/60 dark:hover:border-stone-600 dark:hover:bg-stone-800"
        >
            <span className="grid size-9 place-items-center rounded-xl bg-white text-stone-700 shadow-sm [&>svg]:size-4 dark:bg-stone-900 dark:text-stone-200">{icon}</span>
            <span className="mt-auto text-sm font-medium">{title}</span>
            <span className="mt-1 text-xs text-stone-500 dark:text-stone-400">{description}</span>
        </button>
    );
}
