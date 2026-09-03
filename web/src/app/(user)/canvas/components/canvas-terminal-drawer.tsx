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
    const [isReady, setIsReady] = useState(false);

    const initTerminalSession = async () => {
        if (!containerRef.current || !isTerminalAvailable()) return;

        setIsSpawning(true);
        // 清理已有终端实例
        if (terminalRef.current) {
            terminalRef.current.dispose();
            terminalRef.current = null;
        }

        const resolvedCwd = resolveCaseProjectCwd(projectTitle, projectId);
        setCwd(resolvedCwd);

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
        });

        const fitAddon = new FitAddon();
        term.loadAddon(fitAddon);

        containerRef.current.innerHTML = "";
        term.open(containerRef.current);
        fitAddon.fit();

        terminalRef.current = term;
        fitAddonRef.current = fitAddon;

        const sessionId = sessionIdRef.current;

        // 启动本地系统 PTY (Option A 路径)
        const success = await spawnPty({
            session_id: sessionId,
            cwd: resolvedCwd,
            cols: term.cols,
            rows: term.rows,
        });

        if (success) {
            setIsReady(true);
            term.onData((data) => {
                void writePty(sessionId, data);
            });
        }

        setIsSpawning(false);
    };

    useEffect(() => {
        let unlistenData: (() => void) | undefined;
        let unlistenExit: (() => void) | undefined;

        if (isTerminalAvailable()) {
            void initTerminalSession();

            void onPtyData((payload) => {
                if (payload.session_id === sessionIdRef.current && terminalRef.current) {
                    terminalRef.current.write(payload.data);
                }
            }).then((fn) => {
                unlistenData = fn;
            });

            void onPtyExit((payload) => {
                if (payload.session_id === sessionIdRef.current && terminalRef.current) {
                    terminalRef.current.writeln("\r\n\x1b[33m[进程已退出，点击右上角重启]\x1b[0m");
                }
            }).then((fn) => {
                unlistenExit = fn;
            });
        }

        const handleResize = () => {
            if (fitAddonRef.current && terminalRef.current && isTerminalAvailable()) {
                fitAddonRef.current.fit();
                void resizePty(sessionIdRef.current, terminalRef.current.cols, terminalRef.current.rows);
            }
        };

        window.addEventListener("resize", handleResize);

        return () => {
            window.removeEventListener("resize", handleResize);
            unlistenData?.();
            unlistenExit?.();
            if (terminalRef.current) {
                terminalRef.current.dispose();
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
    };

    const handleClear = () => {
        terminalRef.current?.clear();
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
            <div className="relative flex-1 overflow-hidden p-2">
                <div ref={containerRef} className="h-full w-full" />
            </div>
        </div>
    );
}
