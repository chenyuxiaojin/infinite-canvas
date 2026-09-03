"use client";

import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { nanoid } from "nanoid";
import { Button, Tag, Tooltip } from "antd";
import { Bot, CircleAlert, CircleCheck, Copy, Folder, RefreshCw, Terminal as TerminalIcon, Trash2 } from "lucide-react";

import { cn } from "@/lib/utils";
import {
    isTerminalAvailable,
    onPtyData,
    onPtyExit,
    resizePty,
    resolveCanvasProjectWorkspace,
    spawnPty,
    terminatePty,
    writePty,
} from "@/services/desktop-terminal";
import type { CanvasNodeData } from "../types";

type CanvasTerminalDrawerProps = {
    projectId?: string;
    projectTitle?: string;
    selectedNodes?: CanvasNodeData[];
};

export function CanvasTerminalDrawer({
    projectId,
    projectTitle,
    selectedNodes = [],
}: CanvasTerminalDrawerProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const terminalRef = useRef<Terminal | null>(null);
    const fitAddonRef = useRef<FitAddon | null>(null);
    const sessionIdRef = useRef<string>(`term-${nanoid(8)}`);
    const [cwd, setCwd] = useState<string>("");
    const [isSpawning, setIsSpawning] = useState(false);
    const [spawnError, setSpawnError] = useState<string | null>(null);
    const [workspaceConfigured, setWorkspaceConfigured] = useState(false);
    const [agentCommand, setAgentCommand] = useState<string | null>(null);

    const initTerminalSession = async () => {
        if (!containerRef.current || !isTerminalAvailable()) return;

        setIsSpawning(true);
        setSpawnError(null);

        // 1. 清理已有终端实例
        if (terminalRef.current) {
            terminalRef.current.dispose();
            terminalRef.current = null;
        }

        const workspace = await resolveCanvasProjectWorkspace(projectId, projectTitle).catch(() => null);
        const resolvedCwd = workspace?.projectDirectory || "/Users/chenhuajin/项目/视频制作台/AI编导";
        setCwd(resolvedCwd);
        setWorkspaceConfigured(workspace?.configured === true);
        setAgentCommand(workspace?.agentCommand || null);

        // 2. 初始化 Xterm 实例
        const term = new Terminal({
            theme: {
                background: "#0c0a09", // stone-950
                foreground: "#f5f5f4", // stone-100
                cursor: "#10b981",     // emerald-500
                selectionBackground: "rgba(16, 185, 129, 0.3)",
                black: "#1c1917",
                red: "#ef4444",
                green: "#10b981",
                yellow: "#f59e0b",
                blue: "#3b82f6",
                magenta: "#ec4899",
                cyan: "#06b6d4",
                white: "#f5f5f4",
            },
            fontSize: 12,
            fontFamily: "Menlo, Monaco, 'Courier New', monospace",
            cursorBlink: true,
            allowProposedApi: true,
            convertEol: true,
            scrollback: 5000,
        });

        const fitAddon = new FitAddon();
        term.loadAddon(fitAddon);

        containerRef.current.innerHTML = "";
        term.open(containerRef.current);

        // 等待下一帧让 DOM 布局尺寸生效再 fit
        requestAnimationFrame(() => {
            try {
                fitAddon.fit();
                term.focus();
            } catch {}
        });

        terminalRef.current = term;
        fitAddonRef.current = fitAddon;

        const sessionId = sessionIdRef.current;

        // 3. 监听键盘按键输入写入 PTY
        term.onData((data) => {
            void writePty(sessionId, data);
        });

        // 4. 调用后端创建系统 PTY 进程
        try {
            const cols = term.cols > 0 ? term.cols : 80;
            const rows = term.rows > 0 ? term.rows : 24;

            const ok = await spawnPty({
                session_id: sessionId,
                cwd: resolvedCwd,
                cols,
                rows,
            });

            if (!ok) {
                throw new Error("PTY 返回失败");
            }
        } catch (error) {
            const errorMsg = error instanceof Error ? error.message : String(error);
            console.error("PTY spawn error:", error);
            setSpawnError(errorMsg);
            term.writeln(`\r\n\x1b[31m[终端启动异常: ${errorMsg}]\x1b[0m\r\n`);
        } finally {
            setIsSpawning(false);
        }
    };

    useEffect(() => {
        let unlistenData: (() => void) | undefined;
        let unlistenExit: (() => void) | undefined;

        if (isTerminalAvailable()) {
            // 关键：在 spawn 之前提前挂载数据监听器，防止初次启动的 shell prompt 被丢失
            const setupListeners = async () => {
                const fnData = await onPtyData((payload) => {
                    if (payload.session_id === sessionIdRef.current && terminalRef.current) {
                        terminalRef.current.write(payload.data);
                    }
                });
                unlistenData = fnData;

                const fnExit = await onPtyExit((payload) => {
                    if (payload.session_id === sessionIdRef.current && terminalRef.current) {
                        terminalRef.current.writeln("\r\n\x1b[33m[Shell 进程已退出，点击右上角重启]\x1b[0m");
                    }
                });
                unlistenExit = fnExit;

                await initTerminalSession();
            };

            void setupListeners();
        }

        // 监听外部调整抽屉大小
        const resizeObserver = new ResizeObserver(() => {
            if (fitAddonRef.current && terminalRef.current && isTerminalAvailable()) {
                try {
                    fitAddonRef.current.fit();
                    const cols = terminalRef.current.cols > 0 ? terminalRef.current.cols : 80;
                    const rows = terminalRef.current.rows > 0 ? terminalRef.current.rows : 24;
                    void resizePty(sessionIdRef.current, cols, rows);
                } catch {}
            }
        });

        if (containerRef.current) {
            resizeObserver.observe(containerRef.current);
        }

        return () => {
            resizeObserver.disconnect();
            unlistenData?.();
            unlistenExit?.();
            if (terminalRef.current) {
                terminalRef.current.dispose();
                terminalRef.current = null;
            }
            if (isTerminalAvailable()) {
                void terminatePty(sessionIdRef.current);
            }
        };
    }, [projectId, projectTitle]);

    const handleInjectSelectedNodes = () => {
        if (!selectedNodes.length || !terminalRef.current) return;
        const asSingleLine = (value: string, limit: number) => value
            .replace(/[\u0000-\u001f\u007f]/g, " ")
            .replace(/\s+/g, " ")
            .trim()
            .slice(0, limit);
        const descriptions = selectedNodes
            .map((n) => {
                const meta = n.metadata as Record<string, unknown> | undefined;
                const localMedia = meta?.localMedia as Record<string, unknown> | undefined;
                const title = asSingleLine(n.title || "未命名", 160);
                const content = typeof meta?.content === "string" ? meta.content : typeof meta?.prompt === "string" ? meta.prompt : "";
                const relativePath = typeof localMedia?.relativePath === "string"
                    ? asSingleLine(`${cwd}/agent-media/${localMedia.relativePath}`, 600)
                    : "";
                return [
                    `节点「${title}」`,
                    `类型 ${n.type}`,
                    `ID ${n.id}`,
                    content ? `内容 ${asSingleLine(content, 500)}` : "",
                    relativePath ? `本地素材 ${relativePath}` : "",
                ].filter(Boolean).join("，");
            })
            .join("；");

        const prompt = `请结合无限画布当前选中的 ${selectedNodes.length} 个节点继续处理：${descriptions} `;
        void writePty(
            sessionIdRef.current,
            `\u001b[200~${prompt}\u001b[201~`,
        );
        terminalRef.current.focus();
    };

    const handleStartAgent = (command: "codex" | "claude") => {
        if (!terminalRef.current) return;
        const shellQuote = (value: string) => `'${value.replace(/'/g, `'"'"'`)}'`;
        const launchCommand = command === "codex" && agentCommand
            ? [
                "codex",
                "-c", shellQuote(`mcp_servers.infinite_canvas.command=${JSON.stringify(agentCommand)}`),
                "-c", shellQuote('mcp_servers.infinite_canvas.args=["mcp","serve"]'),
                "-c", shellQuote("mcp_servers.infinite_canvas.enabled=true"),
            ].join(" ")
            : command;
        void writePty(sessionIdRef.current, `${launchCommand}\r`);
        terminalRef.current.focus();
    };

    const handleClear = () => {
        terminalRef.current?.clear();
        terminalRef.current?.focus();
    };

    const handleRestart = async () => {
        await terminatePty(sessionIdRef.current).catch(() => undefined);
        sessionIdRef.current = `term-${nanoid(8)}`;
        await initTerminalSession();
    };

    if (!isTerminalAvailable()) {
        return (
            <div className="flex h-full flex-col items-center justify-center p-6 text-center text-stone-400">
                <TerminalIcon className="size-12 text-stone-600 mb-3" />
                <h3 className="text-sm font-medium text-stone-300">系统终端模式</h3>
                <p className="mt-2 text-xs text-stone-500 max-w-xs leading-relaxed">
                    原生交互终端仅在 <strong>macOS 桌面原生应用</strong> 中启用。
                    支持直接在右侧运行 Claude Code、Codex CLI 以及本地 TUI 智能体，并与左侧画布双向联动。
                </p>
            </div>
        );
    }

    return (
        <div className="flex h-full flex-col bg-stone-950 text-stone-200">
            <div className="border-b border-stone-800 bg-stone-900/70 px-3 py-2.5 text-xs">
                <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 overflow-hidden">
                    <Folder className="size-3.5 text-emerald-400 shrink-0" />
                    <Tooltip title={cwd}>
                        <span className="truncate font-mono text-[11px] text-stone-300">
                            {cwd ? cwd.split("/").slice(-2).join("/") : "定位中..."}
                        </span>
                    </Tooltip>
                    <Tooltip title={workspaceConfigured ? "当前片子已连接画布，AI 可以读取和更新节点" : "终端可用，但这个目录尚未连接画布"}>
                        <span className={cn("inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[10px]", workspaceConfigured ? "bg-emerald-500/15 text-emerald-300" : "bg-amber-500/15 text-amber-300")}>
                            {workspaceConfigured ? <CircleCheck className="size-3" /> : <CircleAlert className="size-3" />}
                            {workspaceConfigured ? "画布已连接" : "未连接画布"}
                        </span>
                    </Tooltip>
                    {spawnError && (
                        <Tag color="error" className="m-0 text-[10px] py-0 px-1">启动异常</Tag>
                    )}
                </div>

                <div className="flex items-center gap-1">
                    {selectedNodes.length > 0 && (
                        <Tooltip title={`把选中的 ${selectedNodes.length} 个节点放进 AI 输入框`}>
                            <Button
                                size="small"
                                type="text"
                                className="text-stone-300 hover:text-emerald-400"
                                icon={<Copy className="size-3.5" />}
                                onClick={handleInjectSelectedNodes}
                                aria-label="把选中节点放进 AI 输入框"
                            />
                        </Tooltip>
                    )}
                    <Tooltip title="清屏">
                        <Button
                            size="small"
                            type="text"
                            className="text-stone-400 hover:text-stone-200"
                            icon={<Trash2 className="size-3.5" />}
                            onClick={handleClear}
                        />
                    </Tooltip>
                    <Tooltip title="重启终端会话">
                        <Button
                            size="small"
                            type="text"
                            className="text-stone-400 hover:text-stone-200"
                            icon={<RefreshCw className={cn("size-3.5", isSpawning && "animate-spin")} />}
                            onClick={() => void handleRestart()}
                        />
                    </Tooltip>
                </div>
                </div>

                <div className="mt-2 flex items-center gap-2">
                    <span className="shrink-0 text-[11px] text-stone-500">启动本地 AI</span>
                    <Button size="small" icon={<Bot className="size-3.5" />} onClick={() => handleStartAgent("codex")}>
                        Codex
                    </Button>
                    <Button size="small" icon={<Bot className="size-3.5" />} onClick={() => handleStartAgent("claude")}>
                        Claude
                    </Button>
                    {selectedNodes.length > 0 ? (
                        <Button size="small" type="primary" icon={<Copy className="size-3.5" />} onClick={handleInjectSelectedNodes} className="ml-auto bg-emerald-600">
                            放入 {selectedNodes.length} 个节点
                        </Button>
                    ) : (
                        <span className="ml-auto text-[11px] text-stone-500">先在左侧选节点，可直接带给 AI</span>
                    )}
                </div>

                {selectedNodes.length > 0 ? (
                    <div className="mt-2 flex gap-1 overflow-x-auto pb-0.5">
                        {selectedNodes.slice(0, 6).map((node) => (
                            <span key={node.id} className="max-w-40 shrink-0 truncate rounded-md bg-stone-800 px-2 py-1 text-[10px] text-stone-300">
                                {node.title || "未命名节点"}
                            </span>
                        ))}
                        {selectedNodes.length > 6 ? <span className="shrink-0 px-1 py-1 text-[10px] text-stone-500">+{selectedNodes.length - 6}</span> : null}
                    </div>
                ) : null}
            </div>

            {/* 终端字符流容器 */}
            <div className="relative flex-1 overflow-hidden p-2 min-h-0 bg-stone-950" onClick={() => terminalRef.current?.focus()}>
                <div ref={containerRef} className="h-full w-full" />
            </div>
        </div>
    );
}
