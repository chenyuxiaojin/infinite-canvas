"use client";

import { App, Button, Spin, Tag } from "antd";
import { CircleStop, Cpu, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { cancelDesktopTask, fetchDesktopTaskStatus, generateDesktopTestClip, isDesktopRuntime, probeDesktopRuntime, type DesktopRuntimeReport, type DesktopTaskSnapshot, type RuntimeStatus } from "@/services/desktop-runtime";

const TERMINAL_TASK_STATES = new Set<DesktopTaskSnapshot["status"]>(["succeeded", "failed", "cancelled"]);

export function DesktopRuntimePanel({ active }: { active: boolean }) {
    const { message } = App.useApp();
    const [desktop, setDesktop] = useState(false);
    const [loading, setLoading] = useState(false);
    const [runningSample, setRunningSample] = useState(false);
    const [report, setReport] = useState<DesktopRuntimeReport | null>(null);
    const [task, setTask] = useState<DesktopTaskSnapshot | null>(null);
    const pollGeneration = useRef(0);
    const autoProbeStarted = useRef(false);

    useEffect(() => setDesktop(isDesktopRuntime()), []);

    const refresh = useCallback(async () => {
        if (!desktop) return;
        setLoading(true);
        try {
            setReport(await probeDesktopRuntime());
        } catch (error) {
            message.error(readError(error, "本地能力探测失败"));
        } finally {
            setLoading(false);
        }
    }, [desktop, message]);

    useEffect(() => {
        if (active && desktop && !report && !autoProbeStarted.current) {
            autoProbeStarted.current = true;
            void refresh();
        }
    }, [active, desktop, refresh, report]);

    useEffect(
        () => () => {
            pollGeneration.current += 1;
        },
        [],
    );

    const runSample = async () => {
        setRunningSample(true);
        const generation = ++pollGeneration.current;
        try {
            const submitted = await generateDesktopTestClip();
            if (submitted.duplicate) message.info("已复用同一幂等验收任务，没有重复生成");
            for (let attempt = 0; attempt < 180 && generation === pollGeneration.current; attempt += 1) {
                const snapshot = await fetchDesktopTaskStatus(submitted.task_id);
                setTask(snapshot);
                if (TERMINAL_TASK_STATES.has(snapshot.status)) {
                    if (snapshot.status === "succeeded") message.success("1 秒本地测试片已生成并完整解码");
                    else if (snapshot.status === "failed") message.error(snapshot.error?.message || "本地测试任务失败");
                    return;
                }
                await delay(250);
            }
            throw new Error("等待本地测试任务超时");
        } catch (error) {
            message.error(readError(error, "本地测试任务失败"));
        } finally {
            if (generation === pollGeneration.current) setRunningSample(false);
        }
    };

    const cancelSample = async () => {
        if (!task || TERMINAL_TASK_STATES.has(task.status)) return;
        try {
            await cancelDesktopTask(task.id);
            message.info("已请求取消本地测试任务");
        } catch (error) {
            message.error(readError(error, "取消失败"));
        }
    };

    const cards = useMemo(() => {
        if (!report) return [];
        return [
            {
                key: "ffmpeg",
                name: "FFmpeg",
                status: report.ffmpeg.status as RuntimeStatus,
                detail: report.ffmpeg.tools.map((tool) => tool.version_line).join(" · ") || report.ffmpeg.diagnostic,
            },
            ...report.connectors.map((provider) => ({
                key: provider.provider,
                name: provider.provider === "eagle" ? "Eagle" : "达芬奇",
                status: provider.status,
                detail: provider.diagnostic,
            })),
            ...report.audio.providers.map((provider) => ({
                key: provider.provider,
                name: provider.display_name,
                status: provider.status,
                detail:
                    provider.service_status === "ready"
                        ? "本地服务身份已确认；本次探测不等同于生成验收"
                        : !provider.installation_found
                          ? `安装路径未授权 · 服务 ${statusLabel(provider.service_status)}`
                          : `模型 ${provider.models_complete ? "完整" : "缺失"} · 服务 ${statusLabel(provider.service_status)}`,
            })),
        ];
    }, [report]);

    return (
        <section className="mb-5 rounded-xl border border-stone-200 bg-stone-50/70 p-4 dark:border-stone-800 dark:bg-stone-900/40">
            <div className="flex items-start justify-between gap-4">
                <div>
                    <div className="flex items-center gap-2 text-sm font-semibold text-stone-900 dark:text-stone-100">
                        <Cpu className="size-4" />
                        本地执行与连接器
                    </div>
                    <div className="mt-1 text-xs leading-5 text-stone-500">只读探测通过 Tauri IPC；网页层不能提交 shell、URL、端口或可执行路径。</div>
                </div>
                {desktop ? (
                    <Button size="small" icon={<RefreshCw className="size-3.5" />} loading={loading} onClick={() => void refresh()}>
                        刷新
                    </Button>
                ) : null}
            </div>

            {!desktop ? (
                <div className="mt-3 rounded-lg border border-dashed border-stone-300 px-3 py-2.5 text-xs text-stone-500 dark:border-stone-700">当前是浏览器模式：本地执行关闭，React/Next.js 仍可独立使用和构建。</div>
            ) : loading && !report ? (
                <div className="flex items-center gap-2 py-6 text-xs text-stone-500">
                    <Spin size="small" />
                    正在读取本机能力，只执行固定只读探测…
                </div>
            ) : (
                <>
                    <div className="mt-3 grid gap-2 sm:grid-cols-2">
                        {cards.map((card) => (
                            <div key={card.key} className="rounded-lg border border-stone-200 bg-white px-3 py-2.5 dark:border-stone-800 dark:bg-stone-950/50">
                                <div className="flex items-center justify-between gap-2">
                                    <span className="text-sm font-medium">{card.name}</span>
                                    <Tag color={statusColor(card.status)} className="!m-0">
                                        {statusLabel(card.status)}
                                    </Tag>
                                </div>
                                <div className="mt-1 line-clamp-2 text-xs leading-5 text-stone-500">{card.detail}</div>
                            </div>
                        ))}
                    </div>

                    {report?.ffmpeg.status === "available" ? (
                        <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-stone-200 pt-3 dark:border-stone-800">
                            <Button size="small" type="primary" loading={runningSample} onClick={() => void runSample()}>
                                生成并验证 1 秒测试片
                            </Button>
                            {task && !TERMINAL_TASK_STATES.has(task.status) ? (
                                <Button size="small" danger icon={<CircleStop className="size-3.5" />} onClick={() => void cancelSample()}>
                                    取消
                                </Button>
                            ) : null}
                            {task ? <TaskReceipt task={task} /> : <span className="text-xs text-stone-500">固定测试图与测试音，零付费，不读取用户素材。</span>}
                        </div>
                    ) : null}
                </>
            )}
        </section>
    );
}

function TaskReceipt({ task }: { task: DesktopTaskSnapshot }) {
    const sha = task.result?.sha256;
    const duration = task.result?.probe.duration_ms;
    const warning = task.error?.side_effects_may_exist ? "；状态未持久化，输出可能存在，需对账" : "";
    return (
        <span className="min-w-0 text-xs text-stone-500">
            任务 {task.id.slice(0, 8)} · {statusLabel(task.status)}
            {duration ? ` · ${(duration / 1000).toFixed(3)} 秒` : ""}
            {sha ? ` · SHA-256 ${sha.slice(0, 12)}…` : ""}
            {warning}
        </span>
    );
}

function statusLabel(status: string) {
    const labels: Record<string, string> = {
        available: "可用",
        unavailable: "未连接",
        not_installed: "未安装",
        not_running: "未运行",
        permission_missing: "缺少权限",
        incompatible: "不兼容",
        ready: "服务就绪",
        discovered: "已发现",
        model_missing: "模型缺失",
        unexpected_response: "身份不匹配",
        not_checked: "未探测",
        queued: "排队中",
        running: "运行中",
        succeeded: "已通过",
        failed: "失败",
        cancelled: "已取消",
        error: "错误",
    };
    return labels[status] || status;
}

function statusColor(status: string) {
    if (["available", "ready", "succeeded"].includes(status)) return "success";
    if (["not_running", "discovered", "queued", "running", "not_checked"].includes(status)) return "processing";
    if (["not_installed", "unavailable", "cancelled"].includes(status)) return "default";
    return "error";
}

function readError(error: unknown, fallback: string) {
    return typeof error === "string" ? error : error instanceof Error ? error.message : fallback;
}

function delay(milliseconds: number) {
    return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}
