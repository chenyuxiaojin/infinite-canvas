"use client";

import { type CSSProperties, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
    History,
    Bot,
    PanelRightClose,
    Plus,
    RotateCcw,
    Sparkles,
    Terminal as TerminalIcon,
    Trash2,
    Video,
    X,
} from "lucide-react";
import { Button, Modal, Tooltip } from "antd";
import { motion } from "motion/react";
import { nanoid } from "nanoid";
import { useCopyText } from "@/hooks/use-copy-text";
import { conversationWindow, conversationMatches, CONVERSATION_PAGE_SIZE, CONVERSATION_PAGE_OVERLAP } from "../utils/canvas-conversation-window";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { CanvasTerminalDrawer } from "./canvas-terminal-drawer";

import { ImageGenerationPending } from "@/components/image-generation-pending";
import { isTauri } from "@tauri-apps/api/core";
import { codexContextPercent } from "@/services/canvas-codex";
import { canvasThemes } from "@/lib/canvas-theme";
import { cn } from "@/lib/utils";
import { resolveCanvasModelReferences } from "@/services/canvas-media";
import { useAssetStore } from "@/stores/use-asset-store";
import { useConfigStore, useEffectiveConfig } from "@/stores/use-config-store";
import { useThemeStore } from "@/stores/use-theme-store";
import { createCanvasAgentState, runCanvasAgent } from "../agent/canvas-agent-runtime";
import { runCanvasCodex } from "../agent/canvas-codex-runtime";
import { runCanvasLocalAgent } from "../agent/canvas-local-agent-runtime";
import type { CanvasAgentContext } from "../agent/canvas-agent-context";
import { isCanvasAgentMediaAction, type CanvasAgentAction, type CanvasAgentToolResult } from "../agent/canvas-agent-tools";
import {
    CanvasNodeType,
    type CanvasAgentConfig,
    type CanvasAgentState,
    type CanvasAssistantMessage,
    type CanvasAssistantReference,
    type CanvasAssistantSession,
    type CanvasNodeData,
} from "../types";
import { assistantReferenceContentFromNode, buildAllCanvasResourceReferences, type CanvasResourceReference } from "../utils/canvas-resource-references";
import { assistantToPromptReference, CanvasAssistantComposer } from "./canvas-assistant-composer";
import { CanvasPromptChipInput } from "./canvas-prompt-chip-input";

const PANEL_MOTION_MS = 500;
const PANEL_MOTION_SECONDS = PANEL_MOTION_MS / 1000;

type CanvasAssistantPanelProps = {
    nodes: CanvasNodeData[];
    selectedNodeIds: Set<string>;
    referenceNodeClick: { nodeId: string | null; version: number };
    sessions: CanvasAssistantSession[];
    activeSessionId: string | null;
    agentConfig: CanvasAgentConfig;
    width: number;
    onWidthChange: (width: number) => void;
    onSessionsChange: (sessions: CanvasAssistantSession[], activeSessionId: string | null) => void;
    onAgentConfigChange: (patch: Partial<CanvasAgentConfig>) => void;
    onPasteImage: (file: File) => void;
    onOpenUpload: () => void;
    onOpenAssets: () => void;
    getAgentContext: (state: CanvasAgentState) => CanvasAgentContext;
    onExecuteAction: (action: CanvasAgentAction, messageReferenceNodeIds: string[]) => Promise<CanvasAgentToolResult>;
    onCollapseStart: () => void;
    onCollapse: () => void;
    initialRequest?: { prompt: string; references: CanvasAssistantReference[] } | null;
    onInitialRequestConsumed?: () => void;
    projectId?: string;
    projectTitle?: string;
    initialView?: "chat" | "terminal";
};

type PendingDeleteConfirmation = {
    permission?: boolean;
    title: string;
    media?: boolean;
    resolve: (confirmed: boolean) => void;
};

export function CanvasAssistantPanel({
    nodes,
    selectedNodeIds,
    referenceNodeClick,
    sessions,
    activeSessionId,
    agentConfig,
    width,
    onWidthChange,
    onSessionsChange,
    onAgentConfigChange,
    onPasteImage,
    onOpenUpload,
    onOpenAssets,
    getAgentContext,
    onExecuteAction,
    onCollapseStart,
    onCollapse,
    initialRequest,
    onInitialRequestConsumed,
    projectId,
    projectTitle,
    initialView = "chat",
}: CanvasAssistantPanelProps) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const effectiveConfig = useEffectiveConfig();
    const isAiConfigReady = useConfigStore((state) => state.isAiConfigReady);
    const cleanupImages = useAssetStore((state) => state.cleanupImages);
    const abortRef = useRef<AbortController | null>(null);
    const consumedInitialRequestRef = useRef<typeof initialRequest>(null);
    const pendingDeleteRef = useRef<PendingDeleteConfirmation | null>(null);
    const messageListRef = useRef<HTMLDivElement>(null);
    const consumedReferenceNodeClickVersionRef = useRef(0);
    const [view, setView] = useState<"chat" | "terminal" | "history">(initialView);
    const [prompt, setPrompt] = useState("");
    const [isRunning, setIsRunning] = useState(false);
    const [checkedChatIds, setCheckedChatIds] = useState<string[]>([]);
    const [deleteChatIds, setDeleteChatIds] = useState<string[]>([]);
    const [closing, setClosing] = useState(false);
    const [resizing, setResizing] = useState(false);
    const [composerReferenceIds, setComposerReferenceIds] = useState<string[]>([]);
    const [removedReferenceIds, setRemovedReferenceIds] = useState<Set<string>>(new Set());
    const [pendingDelete, setPendingDelete] = useState<PendingDeleteConfirmation | null>(null);
    const [initialSession] = useState(() => createSession(isTauri() ? "codex" : "api"));
    const safeSessions = sessions.length ? sessions : [initialSession];
    const resolvedActiveSessionId = activeSessionId && safeSessions.some((session) => session.id === activeSessionId) ? activeSessionId : safeSessions[0]?.id || null;
    const sessionsRef = useRef<CanvasAssistantSession[]>(safeSessions);
    const activeSessionIdRef = useRef<string | null>(resolvedActiveSessionId);

    useEffect(() => {
        sessionsRef.current = safeSessions;
        activeSessionIdRef.current = resolvedActiveSessionId;
    }, [resolvedActiveSessionId, sessions]);

    useEffect(() => () => {
        abortRef.current?.abort();
        pendingDeleteRef.current?.resolve(false);
        pendingDeleteRef.current = null;
    }, []);

    const activeSession = safeSessions.find((session) => session.id === resolvedActiveSessionId) || safeSessions[0] || null;
    const provider = activeSession?.provider || "api";
    const contextPercent = codexContextPercent(activeSession?.codexUsage);
    const historySessions = safeSessions.filter((session) => session.messages.length > 0);
    const messages = activeSession?.messages || [];
    const copyText = useCopyText();
    const [historyEndId, setHistoryEndId] = useState<string | null>(null);
    const [historyQuery, setHistoryQuery] = useState("");
    const [historyMatch, setHistoryMatch] = useState(0);
    const [historyNavigation, setHistoryNavigation] = useState(0);
    const historyPage = useMemo(() => conversationWindow(messages, historyEndId), [messages, historyEndId]);
    const historyMatches = useMemo(() => conversationMatches(messages, historyQuery), [messages, historyQuery]);
    const pendingHistoryScroll = useRef<string | "top" | "bottom" | null>(null);
    const hasMessages = messages.length > 0;
    const selectedNodeKey = useMemo(() => Array.from(selectedNodeIds).sort().join(","), [selectedNodeIds]);

    const followMessagesRef = useRef(true);
    const [followingMessages, setFollowingMessages] = useState(true);
    const jumpToLatest = useCallback(() => {
        setHistoryEndId(null);
        pendingHistoryScroll.current = "bottom";
        setHistoryNavigation((value) => value + 1);
        followMessagesRef.current = true;
        setFollowingMessages(true);
        const element = messageListRef.current;
        if (element) element.scrollTop = element.scrollHeight;
    }, []);
    useEffect(() => { setHistoryQuery(""); setHistoryMatch(0); jumpToLatest(); }, [resolvedActiveSessionId, view, jumpToLatest]);
    useLayoutEffect(() => {
        if (view !== "chat") return;
        const element = messageListRef.current;
        if (!element) return;
        const target = pendingHistoryScroll.current;
        pendingHistoryScroll.current = null;
        if (target === "top") element.scrollTop = 0;
        else if (target && target !== "bottom") element.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(target)}"]`)?.scrollIntoView({ block: "center" });
        else if (target === "bottom" || (historyPage.latest && followMessagesRef.current)) element.scrollTop = element.scrollHeight;
    }, [messages, view, historyEndId, historyPage.latest, historyNavigation]);
    const showHistoryPage = (end: number, scroll: "top" | "bottom" | string) => {
        followMessagesRef.current = false;
        setFollowingMessages(false);
        pendingHistoryScroll.current = scroll;
        setHistoryNavigation((value) => value + 1);
        setHistoryEndId(messages[Math.max(0, Math.min(messages.length, end) - 1)]?.id || null);
    };
    const showHistoryMatch = (offset: number) => {
        if (!historyMatches.length) return;
        const match = (offset + historyMatches.length) % historyMatches.length;
        setHistoryMatch(match);
        const index = historyMatches[match];
        showHistoryPage(Math.min(messages.length, index + Math.ceil(CONVERSATION_PAGE_SIZE / 2)), messages[index].id);
    };
    useEffect(() => {
        const holdSelection = () => {
            const selection = window.getSelection();
            if (!selection || selection.isCollapsed || !messageListRef.current?.contains(selection.anchorNode)) return;
            followMessagesRef.current = false;
            setFollowingMessages(false);
            setHistoryEndId((current) => current || messages.at(-1)?.id || null);
        };
        document.addEventListener("selectionchange", holdSelection);
        return () => document.removeEventListener("selectionchange", holdSelection);
    }, [messages]);
    const resourceReferences = useMemo(() => buildAllCanvasResourceReferences(nodes), [nodes]);
    const resourceReferenceById = useMemo(() => new Map(resourceReferences.map((reference) => [reference.nodeId, reference])), [resourceReferences]);
    const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
    const resolveReferences = useCallback((ids: string[]) => ids.flatMap((id) => {
        const node = nodeById.get(id);
        const resource = resourceReferenceById.get(id);
        const reference = node && resource ? nodeToReference(node, resource) : null;
        return reference ? [reference] : [];
    }), [nodeById, resourceReferenceById]);
    const composerReferences = useMemo(() => resolveReferences(composerReferenceIds), [composerReferenceIds, resolveReferences]);
    const pendingReferences = useMemo(() => {
        const pendingClickNodeId = referenceNodeClick.version > consumedReferenceNodeClickVersionRef.current ? referenceNodeClick.nodeId : null;
        return resourceReferences.filter(
            (reference) => selectedNodeIds.has(reference.nodeId) && ((!composerReferenceIds.includes(reference.nodeId) && !removedReferenceIds.has(reference.nodeId)) || reference.nodeId === pendingClickNodeId),
        );
    }, [composerReferenceIds, referenceNodeClick, removedReferenceIds, resourceReferences, selectedNodeIds]);
    const iconButtonStyle = { color: theme.node.muted };
    const settleDeleteConfirmation = (confirmed: boolean) => {
        const pending = pendingDeleteRef.current;
        if (!pending) return;
        pendingDeleteRef.current = null;
        setPendingDelete(null);
        pending.resolve(confirmed);
    };

    useEffect(() => {
        setRemovedReferenceIds(new Set());
    }, [selectedNodeKey]);

    const commitSessions = (nextSessions: CanvasAssistantSession[], nextActiveSessionId = activeSessionIdRef.current) => {
        sessionsRef.current = nextSessions;
        activeSessionIdRef.current = nextActiveSessionId;
        onSessionsChange(nextSessions, nextActiveSessionId);
    };

    const updateSession = (sessionId: string, updater: (session: CanvasAssistantSession) => CanvasAssistantSession) => {
        commitSessions(sessionsRef.current.map((session) => (session.id === sessionId ? updater(session) : session)));
    };

    const appendMessage = (sessionId: string, message: CanvasAssistantMessage) => {
        updateSession(sessionId, (session) => ({
            ...session,
            title: session.messages.length ? session.title : message.text.slice(0, 18) || "新对话",
            messages: [...session.messages, message],
            updatedAt: new Date().toISOString(),
        }));
    };

    const updateMessage = (sessionId: string, messageId: string, patch: Partial<CanvasAssistantMessage>) => {
        updateSession(sessionId, (session) => ({
            ...session,
            messages: session.messages.map((message) => (message.id === messageId ? { ...message, ...patch } : message)),
            updatedAt: new Date().toISOString(),
        }));
    };

    const startChatSession = () => {
        if (isRunning) return;
        if (activeSession && activeSession.messages.length === 0) {
            commitSessions(sessionsRef.current, activeSession.id);
            return;
        }
        const session = createSession(provider);
        commitSessions([session, ...sessionsRef.current], session.id);
    };

    const removeSessions = (ids: string[]) => {
        const next = safeSessions.filter((session) => !ids.includes(session.id));
        if (!next.length) {
            const session = createSession();
            commitSessions([session], session.id);
        } else {
            const currentActiveSessionId = activeSessionIdRef.current;
            commitSessions(next, currentActiveSessionId && ids.includes(currentActiveSessionId) ? next[0].id : currentActiveSessionId);
        }
        cleanupImages({ sessions: next });
        setCheckedChatIds((previous) => previous.filter((id) => !ids.includes(id)));
    };

    const clearSessions = () => {
        const session = createSession();
        commitSessions([session], session.id);
        setCheckedChatIds([]);
        cleanupImages({ sessions: [session] });
    };

    const sendMessage = async (text: string, savedReferences?: CanvasAssistantReference[]) => {
        if (abortRef.current) return;
        const session = activeSession || createSession();
        if (!activeSession) {
            commitSessions([session], session.id);
        }

        const references = savedReferences || composerReferences;
        const messageReferenceNodeIds = references.map((reference) => reference.id);
        const userMessage: CanvasAssistantMessage = { id: nanoid(), role: "user", text, references, status: "success" };
        const assistantId = nanoid();
        appendMessage(session.id, userMessage);
        appendMessage(session.id, { id: assistantId, role: "assistant", text: "", status: "thinking", activity: "正在理解画布和创作目标" });
        setPrompt("");
        setComposerReferenceIds([]);
        setRemovedReferenceIds(new Set(selectedNodeIds));

        const requestConfig = {
            ...effectiveConfig,
            model: effectiveConfig.textModel || effectiveConfig.model,
            activeChannelId: effectiveConfig.textChannelId || effectiveConfig.activeChannelId,
            textChannelId: effectiveConfig.textChannelId,
        };
        if (provider === "api" && !isAiConfigReady(requestConfig, requestConfig.model)) {
            updateMessage(session.id, assistantId, {
                text: "全局文本模型尚未配置完成。请先从应用原有的全局配置入口选择文本模型和渠道，然后再继续。",
                status: "error",
                activity: undefined,
            });
            return;
        }

        const controller = new AbortController();
        abortRef.current = controller;
        setIsRunning(true);
        let confirmationQueue = Promise.resolve();
        const requestConfirmation = (details: Omit<PendingDeleteConfirmation, "resolve">): Promise<boolean> => {
            const result = confirmationQueue.then(() => {
                if (controller.signal.aborted) return false;
                return new Promise<boolean>((resolve) => {
                    const pending = { ...details, resolve };
                    pendingDeleteRef.current = pending;
                    setPendingDelete(pending);
                });
            });
            confirmationQueue = result.then(() => {});
            return result;
        };
        try {
            const modelReferences = await resolveCanvasModelReferences(projectId || "", references);
            const runtimeInput = {
                config: requestConfig,
                initialState: session.agentState,
                protocolMessages: session.protocolMessages,
                userText: text,
                references: modelReferences,
                getContext: getAgentContext,
                executeAction: async (action: CanvasAgentAction): Promise<CanvasAgentToolResult> => {
                    if (controller.signal.aborted) throw new DOMException("已停止", "AbortError");
                    const media = provider !== "api" && isCanvasAgentMediaAction(action);
                    const connectionDelete = provider !== "api" && action.name === "delete_connection";
                    if (action.name !== "delete_node" && !media && !connectionDelete) return onExecuteAction(action, messageReferenceNodeIds);
                    const nodeId = typeof action.arguments.nodeId === "string" ? action.arguments.nodeId : "";
                    const node = nodes.find((item) => item.id === nodeId);
                    const confirmed = await requestConfirmation({ title: media ? String(action.arguments.title || action.arguments.prompt || "媒体生成").slice(0, 80) : connectionDelete ? `连线 ${action.arguments.connectionId}` : node?.title || "未命名节点", media });
                    if (controller.signal.aborted) throw new DOMException("已停止", "AbortError");
                    return confirmed ? onExecuteAction(action, messageReferenceNodeIds) : { ok: false, code: "action_cancelled", message: "用户取消本次操作，不要自动重试" };
                },
                signal: controller.signal,
                onContextTrace: (trace: NonNullable<CanvasAssistantMessage["contextTrace"]>[number]) => updateSession(session.id, (current) => ({
                    ...current,
                    messages: current.messages.map((message) => message.id === assistantId ? { ...message, contextTrace: [...(message.contextTrace || []), trace] } : message),
                })),
                onEvent: (event: { status: CanvasAssistantMessage["status"]; label: string }) => updateMessage(session.id, assistantId, { status: event.status, activity: event.label }),
                onCheckpoint: (checkpoint: { state: CanvasAgentState; protocolMessages: CanvasAssistantSession["protocolMessages"] }) =>
                    updateSession(session.id, (current) => ({
                        ...current,
                        agentState: checkpoint.state,
                        protocolMessages: checkpoint.protocolMessages,
                        updatedAt: new Date().toISOString(),
                    })),
            };
            const result = provider === "codex"
                ? await runCanvasCodex({
                    ...runtimeInput,
                    projectId: projectId || "",
                    sessionId: session.id,
                    threadId: session.codexThreadId,
                    onCodexUpdate: (patch) => updateSession(session.id, (current) => ({ ...current, ...patch })),
                    onText: (reply) => updateMessage(session.id, assistantId, { text: reply }),
                    onInvalidate: () => {
                        controller.abort();
                        settleDeleteConfirmation(false);
                    },
                })
                : provider === "grok" || provider === "antigravity"
                    ? await runCanvasLocalAgent({
                        ...runtimeInput, provider, projectId: projectId || "", sessionId: session.id, resumeId: session.localAgentSessionId,
                        onSession: (id) => updateSession(session.id, (current) => ({ ...current, localAgentSessionId: id })),
                        onModel: (model) => updateSession(session.id, (current) => ({ ...current, localAgentModel: model })),
                        onText: (reply) => updateMessage(session.id, assistantId, { text: reply }),
                        onInvalidate: () => { controller.abort(); settleDeleteConfirmation(false); },
                        onPermission: (title) => requestConfirmation({ title, permission: true }),
                    })
                    : await runCanvasAgent(runtimeInput);
            updateSession(session.id, (current) => ({
                ...current,
                agentState: result.state,
                protocolMessages: result.protocolMessages,
                messages: current.messages.map((message) =>
                    message.id === assistantId ? { ...message, text: result.reply, status: "success", activity: undefined } : message,
                ),
                updatedAt: new Date().toISOString(),
            }));
        } catch (error) {
            const stopped = error instanceof Error && error.name === "AbortError";
            updateMessage(session.id, assistantId, {
                text: stopped ? "已停止继续执行。已经创建的节点和已经提交的媒体任务会保留。" : error instanceof Error ? error.message : "Agent 执行失败",
                status: stopped ? "waiting" : "error",
                activity: undefined,
            });
        } finally {
            controller.abort();
            settleDeleteConfirmation(false);
            if (abortRef.current === controller) abortRef.current = null;
            setIsRunning(false);
        }
    };

    useEffect(() => {
        if (!initialRequest || consumedInitialRequestRef.current === initialRequest) return;
        consumedInitialRequestRef.current = initialRequest;
        onInitialRequestConsumed?.();
        void sendMessage(initialRequest.prompt, initialRequest.references);
    }, [initialRequest, onInitialRequestConsumed]);

    const submit = async (nextPrompt = prompt, referenceIds = composerReferenceIds) => {
        const text = nextPrompt.trim();
        if (!text || isRunning) return;
        await sendMessage(text, resolveReferences(referenceIds));
    };

    const retryMessage = (message: CanvasAssistantMessage) => {
        const index = messages.findIndex((item) => item.id === message.id);
        const user = messages.slice(0, index).findLast((item) => item.role === "user");
        if (user) void sendMessage(user.text, user.references);
    };

    const startResize = () => {
        const move = (event: MouseEvent) => onWidthChange(Math.min(760, Math.max(320, window.innerWidth - event.clientX)));
        const stop = () => {
            setResizing(false);
            document.body.style.cursor = "";
            document.body.style.userSelect = "";
            document.removeEventListener("mousemove", move);
            document.removeEventListener("mouseup", stop);
        };
        setResizing(true);
        document.body.style.cursor = "col-resize";
        document.body.style.userSelect = "none";
        document.addEventListener("mousemove", move);
        document.addEventListener("mouseup", stop);
    };

    const collapse = () => {
        setClosing(true);
        onCollapseStart();
        window.setTimeout(onCollapse, PANEL_MOTION_MS);
    };

    return (
        <motion.div
            className="flex shrink-0"
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: closing ? 0 : width + 1, opacity: closing ? 0 : 1 }}
            transition={{ duration: resizing ? 0 : PANEL_MOTION_SECONDS, ease: [0.22, 1, 0.36, 1] }}
            style={{ overflow: "clip", pointerEvents: closing ? "none" : undefined }}
        >
            <motion.aside
                data-canvas-agent-panel
                className="relative flex shrink-0 flex-col border-l"
                initial={{ x: 48 }}
                animate={{ x: closing ? 28 : 0 }}
                transition={{ duration: resizing ? 0 : PANEL_MOTION_SECONDS, ease: [0.22, 1, 0.36, 1] }}
                style={{ width, background: theme.node.panel, borderColor: theme.node.stroke, color: theme.node.text }}
            >
                <button type="button" className="absolute inset-y-0 left-0 z-40 w-4 -translate-x-1/2 cursor-col-resize" onMouseDown={startResize} aria-label="调整右侧面板宽度" />
                <div className="flex items-center justify-between border-b px-3 py-2.5" style={{ borderColor: theme.node.stroke }}>
                    <div className="flex items-center gap-1 rounded-lg bg-stone-900/60 p-0.5 ring-1 ring-stone-800">
                        <button
                            type="button"
                            onClick={() => setView("chat")}
                            className={cn(
                                "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium transition cursor-pointer",
                                view === "chat" ? "bg-stone-800 text-white shadow-sm" : "text-stone-400 hover:text-stone-200"
                            )}
                        >
                            <Bot className="size-3.5" />
                            <span>画布 Agent</span>
                        </button>
                        <button
                            type="button"
                            onClick={() => setView("terminal")}
                            className={cn(
                                "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium transition cursor-pointer",
                                view === "terminal" ? "bg-stone-800 text-emerald-400 shadow-sm" : "text-stone-400 hover:text-stone-200"
                            )}
                        >
                            <TerminalIcon className="size-3.5" />
                            <span>终端</span>
                        </button>
                    </div>
                    <div className="flex items-center gap-1">
                        {view === "history" ? (
                            <>
                                <Tooltip title="删除选中">
                                    <Button type="text" shape="circle" className="!h-8 !w-8 !min-w-8" style={iconButtonStyle} icon={<Trash2 className="size-4" />} disabled={!checkedChatIds.length} onClick={() => setDeleteChatIds(checkedChatIds)} />
                                </Tooltip>
                                <Tooltip title="删除全部">
                                    <Button type="text" shape="circle" className="!h-8 !w-8 !min-w-8" style={iconButtonStyle} icon={<X className="size-4" />} disabled={!historySessions.length} onClick={() => setDeleteChatIds(historySessions.map((session) => session.id))} />
                                </Tooltip>
                            </>
                        ) : null}
                        <Tooltip title={view === "history" ? "返回对话" : "历史记录"}>
                            <Button type="text" shape="circle" className="!h-8 !w-8 !min-w-8" style={iconButtonStyle} disabled={isRunning} icon={<History className="size-4" />} onClick={() => setView(view === "history" ? "chat" : "history")} />
                        </Tooltip>
                        {view === "chat" && (
                            <Tooltip title="新对话">
                                <Button
                                    type="text"
                                    shape="circle"
                                    className="!h-8 !w-8 !min-w-8"
                                    style={iconButtonStyle}
                                    icon={<Plus className="size-4" />}
                                    disabled={!hasMessages || isRunning}
                                    onClick={() => {
                                        startChatSession();
                                        setView("chat");
                                    }}
                                />
                            </Tooltip>
                        )}
                        <Tooltip title="收起面板">
                            <Button type="text" shape="circle" className="!h-8 !w-8 !min-w-8" style={iconButtonStyle} icon={<PanelRightClose className="size-4" />} onClick={collapse} />
                        </Tooltip>
                    </div>
                </div>

                {view === "chat" ? (
                    <div className="space-y-1.5 border-b px-3 py-2 text-xs" style={{ borderColor: theme.node.stroke, color: theme.node.muted }}>
                        <div className="flex items-center justify-between gap-2">
                            <div className="flex flex-wrap items-center gap-x-3" aria-label="画布助手来源">
                                {(["api", "codex", "grok", "antigravity"] as const).map((choice) => (
                                    <button key={choice} type="button" disabled={isRunning || (choice !== "api" && !isTauri())} aria-pressed={provider === choice}
                                        className="cursor-pointer border-0 bg-transparent p-0 py-1 disabled:cursor-default disabled:opacity-40"
                                        style={{ color: provider === choice ? theme.node.text : theme.node.muted, fontWeight: provider === choice ? 600 : 400 }}
                                        onClick={() => {
                                            if (choice === provider) return;
                                            const next = createSession(choice);
                                            commitSessions([next, ...sessionsRef.current], next.id);
                                        }}>
                                        {({ api: "API 模型", codex: "本机 Codex", grok: "Grok", antigravity: "Antigravity" })[choice]}
                                    </button>
                                ))}
                            </div>
                            {provider === "codex" ? <span title="最近一次模型调用的输入加输出，除以模型窗口；不是账号余额。未返回上限时不猜百分比。">
                                {contextPercent !== null ? `上下文约 ${contextPercent}%` : activeSession?.codexUsage ? `输入 ${activeSession.codexUsage.inputTokens.toLocaleString()} tokens` : "上下文待测"}
                            </span> : null}
                        </div>
                        {provider === "api" ? <div className="truncate">{effectiveConfig.textModel || effectiveConfig.model || "尚未配置文本模型"}</div> : null}
                        {provider === "grok" || provider === "antigravity" ? <div>{activeSession?.localAgentModel || "模型尚未由本机工具报告"} · 使用本机登录 · 当前接入仅文字与节点 · 媒体生成另行确认</div> : null}
                        {provider === "codex" ? <>
                            <div className="truncate">{activeSession?.codexModel || "使用本机 Codex 的 ChatGPT 登录"} · 媒体生成另行确认</div>
                            {contextPercent !== null && contextPercent >= 70 ? <div role="status" style={{ color: theme.node.text }}>
                                {contextPercent >= 85 ? "上下文接近上限；Codex 可能进行摘要压缩，重要定稿请保存在节点中。" : "上下文已超过 70%，建议完成当前阶段后新建对话。"}
                            </div> : null}
                            {activeSession?.codexCompaction ? <div role="status">{activeSession.codexCompaction}</div> : null}
                        </> : null}
                    </div>
                ) : null}

                {view === "terminal" ? (
                    <div className="min-h-0 flex-1 overflow-hidden">
                        <CanvasTerminalDrawer
                            projectId={projectId}
                            projectTitle={projectTitle}
                            selectedNodes={nodes.filter((n) => selectedNodeIds.has(n.id))}
                        />
                    </div>
                ) : (
                    <div ref={messageListRef} onScroll={(event) => {
                        const element = event.currentTarget;
                        const following = historyPage.end === messages.length && element.scrollHeight - element.scrollTop - element.clientHeight <= 48;
                        if (following) setHistoryEndId(null);
                        else if (historyPage.latest) setHistoryEndId(messages[historyPage.end - 1]?.id || null);
                        followMessagesRef.current = following;
                        setFollowingMessages(following);
                    }} className="thin-scrollbar min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-4">
                        {view === "history" ? (
                            <AssistantHistory
                                sessions={historySessions}
                                activeSession={activeSession}
                                checkedIds={checkedChatIds.filter((id) => historySessions.some((session) => session.id === id))}
                                onToggleChecked={(id, checked) => setCheckedChatIds((previous) => (checked ? [...new Set([...previous, id])] : previous.filter((item) => item !== id)))}
                                onOpen={(id) => {
                                    commitSessions(sessionsRef.current, id);
                                    setView("chat");
                                }}
                                onDelete={(id) => setDeleteChatIds([id])}
                            />
                        ) : messages.length ? (
                            <>
                                {!followingMessages ? <button type="button" onClick={jumpToLatest} className="sticky top-0 z-10 mx-auto block px-3 py-1 text-xs" style={{ background: theme.node.fill, color: theme.node.text }}>回到最新消息 ↓</button> : null}
                                {messages.length > CONVERSATION_PAGE_SIZE ? <div className="space-y-2 text-xs" style={{ color: theme.node.text }}>
                                    <div className="flex items-center gap-2">
                                        <input aria-label="搜索完整对话" placeholder="搜索完整对话" value={historyQuery} onChange={(event) => { setHistoryQuery(event.target.value); setHistoryMatch(0); }} className="min-w-0 flex-1 bg-transparent px-1 py-1 outline-none" />
                                        <button type="button" disabled={!historyMatches.length} onClick={() => showHistoryMatch(historyMatch - 1)}>上一处</button>
                                        <button type="button" disabled={!historyMatches.length} onClick={() => showHistoryMatch(historyMatch)}>定位</button>
                                        <button type="button" disabled={!historyMatches.length} onClick={() => showHistoryMatch(historyMatch + 1)}>下一处</button>
                                    </div>
                                    {historyQuery ? <div>{historyMatches.length} 条消息匹配</div> : null}
                                    <div className="flex flex-wrap items-center gap-3">
                                        <button type="button" disabled={!historyPage.start} onClick={() => showHistoryPage(historyPage.start + CONVERSATION_PAGE_OVERLAP, "bottom")}>更早消息</button>
                                        <span>{historyPage.start + 1}–{historyPage.end} / {messages.length}</span>
                                        <button type="button" disabled={historyPage.end >= messages.length} onClick={() => showHistoryPage(historyPage.end + CONVERSATION_PAGE_SIZE - CONVERSATION_PAGE_OVERLAP, "top")}>更新消息</button>
                                        <button type="button" onClick={() => copyText(messages.map((message) => `${message.role === "user" ? "我" : "助手"}：\n${message.text}`).join("\n\n"), "完整对话已复制")}>复制完整对话</button>
                                    </div>
                                </div> : null}
                                <AssistantMessages messages={historyPage.messages} onRetry={retryMessage} />
                            </>
                        ) : (
                            <div className="flex h-full flex-col items-center justify-center px-8 text-center">
                                <div className="grid size-12 place-items-center rounded-2xl" style={{ background: theme.node.fill }}>
                                    <Sparkles className="size-5" />
                                </div>
                                <div className="mt-4 text-base font-medium">从一个想法开始</div>
                                <div className="mt-2 max-w-[260px] text-sm leading-6 opacity-55">描述故事、宣传片或现有素材，Agent 会与你沟通并直接操作当前画布</div>
                            </div>
                        )}
                    </div>
                )}

                {view === "chat" ? (
                    <>
                        {pendingDelete ? (
                            <div className="mx-2 mb-2 overflow-hidden rounded-xl border" style={{ background: theme.node.fill, borderColor: theme.node.stroke }}>
                                <div className="min-w-0 px-3 py-2.5">
                                    <div className={pendingDelete.permission ? "max-h-48 overflow-auto whitespace-pre-wrap break-all text-sm font-medium" : "truncate text-sm font-medium"}>{pendingDelete.permission ? "允许本次操作" : pendingDelete.media ? "生成" : "删除"}「{pendingDelete.title}」？</div>
                                    <div className="mt-0.5 text-xs opacity-55">{pendingDelete.permission ? "这是 Grok 请求的本次工具权限，请核对操作内容；取消后不会自动重试。" : pendingDelete.media ? "会调用已配置的媒体服务，可能消耗额度；本次确认只执行这一项。" : "相关连线和任务记录将按现有逻辑清理"}</div>
                                </div>
                                <div className="grid grid-cols-2 border-t" style={{ borderColor: theme.node.stroke }}>
                                    <button type="button" className="h-9 cursor-pointer border-0 bg-transparent text-sm" style={{ color: theme.node.text }} onClick={() => settleDeleteConfirmation(false)}>取消</button>
                                    <button type="button" className="h-9 cursor-pointer border-0 border-l bg-transparent text-sm font-medium" style={{ borderColor: theme.node.stroke, color: pendingDelete.media || pendingDelete.permission ? theme.node.text : "#ef4444" }} onClick={() => settleDeleteConfirmation(true)}>{pendingDelete.permission ? "允许一次" : pendingDelete.media ? "确认生成" : "确认删除"}</button>
                                </div>
                            </div>
                        ) : null}
                        <CanvasAssistantComposer
                            prompt={prompt}
                            isRunning={isRunning}
                            references={composerReferences}
                            availableReferences={resourceReferences}
                            pendingReferences={pendingReferences}
                            agentConfig={agentConfig}
                            onAgentConfigChange={onAgentConfigChange}
                            onPromptChange={setPrompt}
                            onReferenceIdsChange={(ids) => {
                                consumedReferenceNodeClickVersionRef.current = referenceNodeClick.version;
                                const removedSelectedIds = composerReferenceIds.filter((id) => selectedNodeIds.has(id) && !ids.includes(id));
                                if (removedSelectedIds.length) setRemovedReferenceIds((previous) => new Set([...previous, ...removedSelectedIds]));
                                setComposerReferenceIds(ids);
                            }}
                            onSubmit={submit}
                            onStop={() => {
                                settleDeleteConfirmation(false);
                                abortRef.current?.abort();
                            }}
                            onOpenUpload={onOpenUpload}
                            onOpenAssets={onOpenAssets}
                            onPasteImage={onPasteImage}
                        />
                    </>
                ) : null}

                <Modal
                    title="删除对话记录？"
                    open={deleteChatIds.length > 0}
                    centered
                    onCancel={() => setDeleteChatIds([])}
                    footer={
                        <>
                            <Button onClick={() => setDeleteChatIds([])}>取消</Button>
                            <Button
                                danger
                                type="primary"
                                onClick={() => {
                                    deleteChatIds.length === historySessions.length ? clearSessions() : removeSessions(deleteChatIds);
                                    setDeleteChatIds([]);
                                }}
                            >
                                删除
                            </Button>
                        </>
                    }
                >
                    <p className="text-sm opacity-60">将删除 {deleteChatIds.length} 条对话记录，此操作不可撤销</p>
                </Modal>
            </motion.aside>
        </motion.div>
    );
}

const ASSISTANT_MARKDOWN_COMPONENTS: Components = {
    a: ({ node: _node, ...props }) => <a {...props} target="_blank" rel="noreferrer" className="font-medium underline underline-offset-4" />,
};

const AssistantMarkdown = memo(function AssistantMarkdown({ children }: { children: string }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];

    return (
        <div
            className={cn(
                "min-w-0 whitespace-normal break-words",
                "[&_p]:my-2 [&_p:first-child]:mt-0 [&_p:last-child]:mb-0",
                "[&_h1]:mb-2 [&_h1]:mt-4 [&_h1]:text-lg [&_h1]:font-semibold [&_h1:first-child]:mt-0",
                "[&_h2]:mb-2 [&_h2]:mt-4 [&_h2]:text-base [&_h2]:font-semibold [&_h2:first-child]:mt-0",
                "[&_h3]:mb-1.5 [&_h3]:mt-3 [&_h3]:font-semibold [&_h3:first-child]:mt-0",
                "[&_h4]:my-2 [&_h4]:font-semibold",
                "[&_ul]:my-2 [&_ul]:list-disc [&_ul]:pl-5 [&_ol]:my-2 [&_ol]:list-decimal [&_ol]:pl-5 [&_li]:my-1",
                "[&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-[color:var(--agent-markdown-border)] [&_blockquote]:pl-3 [&_blockquote]:opacity-80",
                "[&_hr]:my-3 [&_hr]:border-0 [&_hr]:border-t [&_hr]:border-[color:var(--agent-markdown-border)]",
                "[&_code]:rounded [&_code]:bg-[var(--agent-markdown-surface)] [&_code]:px-1.5 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.85em]",
                "[&_pre]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-lg [&_pre]:bg-[var(--agent-markdown-surface)] [&_pre]:p-3",
                "[&_pre_code]:bg-transparent [&_pre_code]:p-0",
                "[&_table]:my-2 [&_table]:w-full [&_table]:border-collapse [&_th]:border-b [&_th]:border-[color:var(--agent-markdown-border)] [&_th]:px-2 [&_th]:py-1.5 [&_th]:text-left [&_td]:border-b [&_td]:border-[color:var(--agent-markdown-border)] [&_td]:px-2 [&_td]:py-1.5",
            )}
            style={
                {
                    "--agent-markdown-surface": theme.toolbar.itemHover,
                    "--agent-markdown-border": theme.node.stroke,
                } as CSSProperties
            }
        >
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={ASSISTANT_MARKDOWN_COMPONENTS} skipHtml>
                {children}
            </ReactMarkdown>
        </div>
    );
});

function AssistantMessages({ messages, onRetry }: { messages: CanvasAssistantMessage[]; onRetry: (message: CanvasAssistantMessage) => void }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];

    return (
        <>
            {messages.map((message) => {
                const running = message.status === "thinking" || message.status === "running";
                return (
                    <div key={message.id} data-message-id={message.id} style={running ? undefined : { contentVisibility: "auto", containIntrinsicSize: "auto 160px" }} className={cn("flex flex-col gap-2", message.role === "user" ? "items-end" : "items-start")}>
                        {message.text ? (
                            <div
                                className="max-w-[88%] whitespace-pre-wrap rounded-2xl px-3 py-2 text-sm leading-6"
                                style={
                                    message.role === "user"
                                        ? { background: theme.toolbar.activeBg, color: theme.toolbar.activeText }
                                        : message.status === "error"
                                            ? { background: theme.node.fill, color: theme.node.text }
                                            : { background: theme.node.fill, color: theme.node.text }
                                }
                            >
                                {message.role === "assistant" ? (
                                    <div className="mb-1 flex items-center gap-1.5 text-xs opacity-60">
                                        <Bot className="size-3.5" />
                                        Agent
                                    </div>
                                ) : null}
                                {message.role === "assistant" ? <AssistantMarkdown>{message.text}</AssistantMarkdown> : <UserMessageContent message={message} />}
                            </div>
                        ) : null}
                        {message.contextTrace?.length ? <details className="max-w-full text-xs opacity-70">
                            <summary className="cursor-pointer">本次实际上下文与 SOP</summary>
                            <p className="my-2">下方是传输或工具返回记录；输入框引用是发送前选择。外部文件 SOP 未读取，不代表已采用。</p>
                            {message.contextTrace.map((trace, index) => <div key={index} className="my-2 break-words">
                                <div>{trace.label}</div>
                                {trace.nodes.map((node, i) => <div key={`${node.id}-${i}`}>{node.title} · {node.id} · {node.detail === "image" ? "附图" : node.detail === "body" ? "正文/提示词" : "目录信息"}</div>)}
                                {trace.sources?.map((source) => <div key={source.source} className="my-1 select-text">{source.source}<br />SHA-256：{source.sha256}</div>)}
                            </div>)}
                        </details> : null}
                        {running ? <ImageGenerationPending compact label={message.activity || "正在执行"} className="w-[250px] rounded-2xl border" /> : null}
                        {message.role === "assistant" && !running && message.text ? (
                            <Button shape="circle" size="small" style={{ borderColor: theme.node.stroke }} icon={<RotateCcw className="size-3.5" />} onClick={() => onRetry(message)} title="重试" />
                        ) : null}
                    </div>
                );
            })}
        </>
    );
}

function AssistantHistory({
    sessions,
    activeSession,
    checkedIds,
    onToggleChecked,
    onOpen,
    onDelete,
}: {
    sessions: CanvasAssistantSession[];
    activeSession: CanvasAssistantSession | null;
    checkedIds: string[];
    onToggleChecked: (id: string, checked: boolean) => void;
    onOpen: (id: string) => void;
    onDelete: (id: string) => void;
}) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];

    return (
        <div className="space-y-1">
            {sessions.map((session) => (
                <div key={session.id} className="group flex items-center gap-2 rounded-lg px-2 py-1.5 transition" style={session.id === activeSession?.id ? { background: theme.node.fill } : undefined}>
                    <input type="checkbox" className="size-4" style={{ accentColor: theme.node.text }} checked={checkedIds.includes(session.id)} onChange={(event) => onToggleChecked(session.id, event.target.checked)} />
                    <button type="button" className="min-w-0 flex-1 text-left text-sm" onClick={() => onOpen(session.id)}>
                        <span className="block truncate">{session.title}</span>
                        <span className="text-xs opacity-50">{session.messages.length} 条消息</span>
                    </button>
                    <Button type="text" shape="circle" size="small" className="opacity-0 transition group-hover:opacity-100" icon={<Trash2 className="size-3.5" />} onClick={() => onDelete(session.id)} title="删除" />
                </div>
            ))}
        </div>
    );
}

function UserMessageContent({ message }: { message: CanvasAssistantMessage }) {
    const references = useMemo(() => message.references?.map(assistantToPromptReference) || [], [message.references]);
    return <CanvasPromptChipInput value={message.text} references={references} onChange={ignorePromptChange} readOnly />;
}

function ignorePromptChange() {}

function nodeToReference(node: CanvasNodeData, resource: CanvasResourceReference): CanvasAssistantReference | null {
    const content = assistantReferenceContentFromNode(node);
    return content ? { id: node.id, type: node.type, title: node.title, label: resource.label, ...content } : null;
}

function createSession(provider: NonNullable<CanvasAssistantSession["provider"]> = "api"): CanvasAssistantSession {
    const now = new Date().toISOString();
    return {
        id: nanoid(),
        provider,
        title: "新对话",
        messages: [],
        agentState: createCanvasAgentState(),
        protocolMessages: [],
        createdAt: now,
        updatedAt: now,
    };
}
