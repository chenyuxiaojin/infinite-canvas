"use client";

import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { nanoid } from "nanoid";
import { Button, Tag, Tooltip } from "antd";
import { Copy, Folder, RefreshCw, Terminal as TerminalIcon, Trash2 } from "lucide-react";

import { cn } from "@/lib/utils";
import {
    isTerminalAvailable,
    onPtyData,
    onPtyExit,
    resizePty,
    resolveCaseProjectCwd,
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

    const initTerminalSession = async () => {
        if (!containerRef.current || !isTerminalAvailable()) return;

        setIsSpawning(true);
        setSpawnError(null);

        // 1. 清理已有终端实例
        if (terminalRef.current) {
            terminalRef.current.dispose();
            terminalRef.current = null;
        }

        const resolvedCwd = resolveCaseProjectCwd(projectTitle, projectId);
        setCwd(resolvedCwd);

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
        const descriptions = selectedNodes
            .map((n) => {
                const meta = n.metadata as Record<string, unknown> | undefined;
                const localMedia = meta?.localMedia as Record<string, unknown> | undefined;
                if (n.type === "image" && typeof localMedia?.fileName === "string") {
                    return `agent-media/verified/${localMedia.fileName}`;
                }
                return n.title || n.id;
            })
            .join(" ");

        void writePty(sessionIdRef.current, ` ${descriptions} `);
        terminalRef.current.focus();
    };

    const handleClear = () => {
        terminalRef.current?.clear();
        terminalRef.current?.focus();
    };

    const handleRestart = () => {
        sessionIdRef.current = `term-${nanoid(8)}`;
        void initTerminalSession();
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
            {/* 顶部工作区与快速控制 */}
            <div className="flex items-center justify-between border-b border-stone-800 bg-stone-900/70 px-3 py-2 text-xs">
                <div className="flex items-center gap-2 overflow-hidden">
                    <Folder className="size-3.5 text-emerald-400 shrink-0" />
                    <Tooltip title={cwd}>
                        <span className="truncate font-mono text-[11px] text-stone-300">
                            {cwd ? cwd.split("/").slice(-2).join("/") : "定位中..."}
                        </span>
                    </Tooltip>
                    <Tag color="emerald" className="m-0 text-[10px] py-0 px-1">Option A</Tag>
                    {spawnError && (
                        <Tag color="error" className="m-0 text-[10px] py-0 px-1">启动异常</Tag>
                    )}
                </div>

                <div className="flex items-center gap-1">
                    {selectedNodes.length > 0 && (
                        <Tooltip title={`将选中的 ${selectedNodes.length} 个节点插入终端`}>
                            <Button
                                size="small"
                                type="text"
                                className="text-stone-300 hover:text-emerald-400"
                                icon={<Copy className="size-3.5" />}
                                onClick={handleInjectSelectedNodes}
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
                            onClick={handleRestart}
                        />
                    </Tooltip>
                </div>
            </div>

            {/* 终端字符流容器 */}
            <div className="relative flex-1 overflow-hidden p-2 min-h-0 bg-stone-950" onClick={() => terminalRef.current?.focus()}>
                <div ref={containerRef} className="h-full w-full" />
            </div>
        </div>
    );
}
