"use client";

import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { App } from "antd";
import { favoritePrompt, fetchPromptDetail, unfavoritePrompt, type Prompt } from "@/services/api/prompts";

export function usePromptActions(enabled = true) {
    const { message, modal } = App.useApp();
    const client = useQueryClient();
    const request = useRef<AbortController | null>(null);
    const [loadingId, setLoadingId] = useState("");
    useEffect(() => () => request.current?.abort(), [enabled]);

    const refresh = async () => {
        await Promise.all([
            client.invalidateQueries({ queryKey: ["prompts"] }),
            client.invalidateQueries({ queryKey: ["canvas-side-prompt-category"] }),
        ]);
    };
    const run = async (item: Prompt, action: (loaded: Prompt) => void | Promise<void>) => {
        if (!enabled || request.current) return;
        const controller = new AbortController();
        request.current = controller;
        setLoadingId(item.id);
        try {
            const loaded = item.prompt ? item : await fetchPromptDetail(item.id, controller.signal);
            if (!controller.signal.aborted) await action(loaded);
            return !controller.signal.aborted;
        } catch (error) {
            if (!controller.signal.aborted) message.error(error instanceof Error ? error.message : "加载提示词失败");
            return false;
        } finally {
            if (request.current === controller) { request.current = null; setLoadingId(""); }
        }
    };
    const save = (item: Prompt) => run(item, async (loaded) => {
        await favoritePrompt(loaded);
        message.success("已收藏到本机，可离线读取全文");
        await refresh();
    });
    const remove = (item: Prompt) => modal.confirm({
        title: "取消本机收藏？",
        content: "将移除此收藏的全文副本，在线目录和原有历史数据不受影响。之后需要联网重新加载。",
        okText: "取消收藏",
        cancelText: "保留",
        onOk: async () => { await unfavoritePrompt(item.id); await refresh(); },
    });
    return { run, save, remove, loadingId };
}
