"use client";

import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { nanoid } from "nanoid";
import { Button, Tooltip } from "antd";
import { Copy, Folder, RefreshCw, Terminal as TerminalIcon, Trash2 } from "lucide-react";

import { canvasThemes } from "@/lib/canvas-theme";
import { useThemeStore } from "@/stores/use-theme-store";
import {
    isTerminalAvailable,
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

function terminalTheme() {
    const theme = canvasThemes[useThemeStore.getState().theme];
    return {
        background: theme.node.panel,
        foreground: theme.node.text,
        cursor: theme.node.text,
        selectionBackground: theme.toolbar.activeBg,
    };
}

export function CanvasTerminalDrawer({ projectId, projectTitle, selectedNodes = [] }: CanvasTerminalDrawerProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const terminalRef = useRef<Terminal | null>(null);
    const titleRef = useRef(projectTitle);
    const themeName = useThemeStore((state) => state.theme);
    const theme = canvasThemes[themeName];
    const [cwd, setCwd] = useState("");
    const [isSpawning, setIsSpawning] = useState(true);
    const [spawnError, setSpawnError] = useState<string | null>(null);
    const [restartKey, setRestartKey] = useState(0);
    titleRef.current = projectTitle;

    useEffect(() => {
        const container = containerRef.current;
        if (!container || !isTerminalAvailable()) return;

        const sessionId = "term-" + nanoid(8);
        let cancelled = false;
        let ready = false;
        let exited = false;
        let resizeFrame = 0;
        let lastCols = 0;
        let lastRows = 0;
        const term = new Terminal({
            theme: terminalTheme(),
            fontSize: 12,
            fontFamily: "Menlo, Monaco, 'Courier New', monospace",
            cursorBlink: true,
            scrollback: 5000,
            disableStdin: true,
        });
        const fitAddon = new FitAddon();
        term.loadAddon(fitAddon);
        term.open(container);
        terminalRef.current = term;
        setIsSpawning(true);
        setSpawnError(null);
        setCwd("");

        const fit = () => {
            if (cancelled || !container.clientWidth || !container.clientHeight) return;
            fitAddon.fit();
            if (ready && (term.cols !== lastCols || term.rows !== lastRows)) {
                lastCols = term.cols;
                lastRows = term.rows;
                void resizePty(sessionId, lastCols, lastRows).catch(() => undefined);
            }
        };
        const scheduleFit = () => {
            if (resizeFrame || cancelled) return;
            resizeFrame = requestAnimationFrame(() => {
                resizeFrame = 0;
                fit();
            });
        };
        const resizeObserver = new ResizeObserver(scheduleFit);
        resizeObserver.observe(container);
        term.onData((data) => {
            // Allow terminal query replies from early shell output; keyboard input stays disabled until ready.
            if (!cancelled && !exited) void writePty(sessionId, data).catch((error) => {
                if (!cancelled) setSpawnError(String(error));
            });
        });

        const start = async () => {
            try {
                const workspace = await resolveCanvasProjectWorkspace(projectId, titleRef.current);
                if (cancelled) return;
                setCwd(workspace.projectDirectory);
                fit();
                lastCols = term.cols;
                lastRows = term.rows;
                const ok = await spawnPty({
                    session_id: sessionId,
                    cwd: workspace.projectDirectory,
                    cols: lastCols,
                    rows: lastRows,
                }, (data, consumed) => {
                    if (!cancelled) term.write(data, consumed);
                }, () => {
                    exited = true;
                    ready = false;
                    if (!cancelled) {
                        term.options.disableStdin = true;
                        term.writeln("\r\n[终端已退出，可点击右上角重新启动]");
                    }
                }, (error) => {
                    exited = true;
                    ready = false;
                    if (!cancelled) {
                        term.options.disableStdin = true;
                        setSpawnError(error);
                    }
                });
                if (cancelled) {
                    await terminatePty(sessionId);
                    return;
                }
                if (!ok) throw new Error("终端启动失败");
                ready = !exited;
                term.options.disableStdin = !ready;
                scheduleFit();
                term.focus();
            } catch (error) {
                if (!cancelled) setSpawnError(error instanceof Error ? error.message : String(error));
            } finally {
                if (!cancelled) setIsSpawning(false);
            }
        };
        void start();

        return () => {
            cancelled = true;
            ready = false;
            cancelAnimationFrame(resizeFrame);
            resizeObserver.disconnect();
            if (terminalRef.current === term) terminalRef.current = null;
            // A late spawn is also terminated above, without leaving an orphaned shell.
            void terminatePty(sessionId).catch(() => undefined);
            term.dispose();
        };
    }, [projectId, restartKey]);

    useEffect(() => {
        if (terminalRef.current) terminalRef.current.options.theme = terminalTheme();
    }, [themeName]);

    const handleInjectSelectedNodes = () => {
        const term = terminalRef.current;
        if (!selectedNodes.length || !term || term.options.disableStdin) return;
        const asSingleLine = (value: string, limit: number) => value
            .replace(/[\u0000-\u001f\u007f]/g, " ")
            .replace(/\s+/g, " ").trim().slice(0, limit);
        const descriptions = selectedNodes.map((node) => {
            const meta = node.metadata as Record<string, unknown> | undefined;
            const localMedia = meta?.localMedia as Record<string, unknown> | undefined;
            const content = typeof meta?.content === "string" ? meta.content : typeof meta?.prompt === "string" ? meta.prompt : "";
            const relativePath = typeof localMedia?.relativePath === "string"
                ? asSingleLine(cwd + "/agent-media/" + localMedia.relativePath, 600) : "";
            return [
                "节点「" + asSingleLine(node.title || "未命名", 160) + "」",
                "类型 " + node.type,
                "ID " + node.id,
                content ? "内容 " + asSingleLine(content, 500) : "",
                relativePath ? "本地素材 " + relativePath : "",
            ].filter(Boolean).join("，");
        }).join("；");
        // Paste only; no automatic Enter, command choice or Agent startup.
        term.paste(asSingleLine("请结合当前画布选中的 " + selectedNodes.length + " 个节点继续处理：" + descriptions, 16000) + " ");
        term.focus();
    };

    if (!isTerminalAvailable()) {
        return (
            <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center" style={{ color: theme.node.muted }}>
                <TerminalIcon className="size-10" />
                <p className="text-sm">终端在 macOS 桌面应用中使用，你可以自行输入命令。</p>
            </div>
        );
    }

    return (
        <div className="flex h-full flex-col" style={{ background: theme.node.panel, color: theme.node.text }}>
            <div className="flex shrink-0 items-center justify-between gap-2 px-3 py-1.5 text-xs">
                <Tooltip title={cwd || "正在定位工作目录"}>
                    <span className="flex min-w-0 items-center gap-2" style={{ color: theme.node.muted }}>
                        <Folder className="size-3.5 shrink-0" />
                        <span className="truncate">{cwd ? cwd.split("/").slice(-2).join("/") : "启动中…"}</span>
                    </span>
                </Tooltip>
                <div className="flex shrink-0 items-center">
                    {selectedNodes.length > 0 && (
                        <Tooltip title="粘贴选中节点的说明，不自动执行">
                            <Button size="small" type="text" style={{ color: theme.node.muted }} icon={<Copy className="size-3.5" />} onClick={handleInjectSelectedNodes} disabled={isSpawning} aria-label="粘贴选中节点" />
                        </Tooltip>
                    )}
                    <Tooltip title="清屏">
                        <Button size="small" type="text" style={{ color: theme.node.muted }} icon={<Trash2 className="size-3.5" />} aria-label="清屏" onClick={() => { terminalRef.current?.clear(); terminalRef.current?.focus(); }} />
                    </Tooltip>
                    <Tooltip title="重启终端（结束当前会话）">
                        <Button size="small" type="text" style={{ color: theme.node.muted }} icon={<RefreshCw className="size-3.5" />} disabled={isSpawning} aria-label="重启终端" onClick={() => setRestartKey((key) => key + 1)} />
                    </Tooltip>
                </div>
            </div>
            {spawnError && <p role="alert" className="px-3 py-2 text-xs">终端异常：{spawnError}</p>}
            <div className="min-h-0 flex-1 overflow-hidden p-2" onClick={() => terminalRef.current?.focus()}>
                <div ref={containerRef} className="h-full w-full" />
            </div>
        </div>
    );
}
