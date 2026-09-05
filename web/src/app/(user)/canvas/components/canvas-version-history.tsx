"use client";

import { useEffect, useRef, useState } from "react";
import { App, Button, Modal, Spin } from "antd";
import { History } from "lucide-react";
import { canvasThemes } from "@/lib/canvas-theme";
import { useThemeStore } from "@/stores/use-theme-store";
import { isDesktopRuntime } from "@/services/desktop-runtime";
import { listCanvasVersions, previewCanvasVersion, type CanvasVersion, type CanvasVersionPreview } from "@/services/api/canvas-history";
import { useCanvasStore } from "../stores/use-canvas-store";

const reasons: Record<string, string> = { initial: "首次保留", initial_save: "首次保存", save: "编辑保存", before_restore: "恢复前保留", restore: "恢复版本" };

export function CanvasVersionHistory({ projectId }: { projectId: string }) {
    const { message } = App.useApp();
    const theme = canvasThemes[useThemeStore((s) => s.theme)];
    const project = useCanvasStore((s) => s.projects.find((p) => p.id === projectId));
    const [open, setOpen] = useState(false);
    const [versions, setVersions] = useState<CanvasVersion[]>([]);
    const [preview, setPreview] = useState<CanvasVersionPreview>();
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState("");
    const generation = useRef(0);
    useEffect(() => {
        const current = ++generation.current;
        setPreview(undefined); setError(""); setVersions([]);
        if (!open) return;
        setBusy(true);
        void listCanvasVersions(projectId).then((items) => { if (current === generation.current) setVersions(items); })
            .catch((e) => { if (current === generation.current) setError(String(e)); })
            .finally(() => { if (current === generation.current) setBusy(false); });
        return () => { generation.current++; };
    }, [projectId, open]);
    if (!isDesktopRuntime()) return null;
    const select = async (sequence: number) => {
        const current = ++generation.current;
        setBusy(true); setError(""); setPreview(undefined);
        try { const result = await previewCanvasVersion(projectId, sequence); if (current === generation.current) setPreview(result); }
        catch (e) { if (current === generation.current) setError(String(e)); }
        finally { if (current === generation.current) setBusy(false); }
    };
    const restore = async () => {
        if (!preview) return;
        const current = ++generation.current;
        setBusy(true); setError("");
        try {
            await useCanvasStore.getState().restoreVersion(projectId, preview.sequence, preview.baseRevision);
            const items = await listCanvasVersions(projectId);
            if (current !== generation.current) return;
            setPreview(undefined); setVersions(items);
            message.success("已恢复，并保留恢复前版本");
        } catch (e) { if (current === generation.current) setError(String(e)); }
        finally { if (current === generation.current) setBusy(false); }
    };
    const changedNodes = preview?.project.nodes.filter((node) => JSON.stringify(project?.nodes.find((n) => n.id === node.id)) !== JSON.stringify(node)) || [];
    return <>
        <button type="button" onClick={() => setOpen(true)} title="版本历史" aria-label="版本历史" className="flex items-center gap-1 px-2 py-1 text-xs opacity-70 hover:opacity-100" style={{ color: theme.node.text }}><History size={14} />版本历史</button>
        <Modal title="版本历史" open={open} onCancel={() => { if (!busy) setOpen(false); }} footer={null} width={760}>
            <p className="mb-3 text-xs" style={{ color: theme.node.muted }}>历史跨重启保留。普通编辑最多每 30 秒记录一次，保留最近 100 个版本、约 64 MB；恢复前后至少保留两个版本。原素材不复制、不清理。</p>
            {error && <p role="alert" className="mb-3">{error}</p>}
            {busy && <Spin size="small" />}
            <div className="grid grid-cols-2 gap-4">
                <div className="max-h-96 overflow-y-auto">
                    {!busy && !versions.length && <p>尚无历史版本；下次保存后开始记录。</p>}
                    {versions.map((version) => <button type="button" key={version.sequence} disabled={busy} onClick={() => void select(version.sequence)} className="block w-full py-2 text-left text-sm" style={{ color: preview?.sequence === version.sequence ? theme.node.text : theme.node.muted }}>
                        {new Date(version.createdAt).toLocaleString()}<br /><span className="text-xs">{reasons[version.reason] || version.reason}{version.restoredFrom ? ` · 来自版本 ${version.restoredFrom}` : ""}</span>
                    </button>)}
                </div>
                <div className="max-h-96 overflow-y-auto">
                    {preview ? <>
                        <p className="mb-2">恢复到「{preview.project.title || "未命名画布"}」时：</p>
                        {preview.changes.map((item) => <p key={item.field} className="text-sm">{item.label}：新增 {item.added}、移除 {item.removed}、修改 {item.changed}</p>)}
                        {changedNodes.map((node) => <details key={node.id} className="my-2 text-xs"><summary>{project?.nodes.some((n) => n.id === node.id) ? "修改" : "恢复"}：{node.title || node.type}</summary>{node.metadata?.storageKey !== project?.nodes.find((n) => n.id === node.id)?.metadata?.storageKey && <p>素材引用已变化（原素材仍保留）</p>}{JSON.stringify(node.position) !== JSON.stringify(project?.nodes.find((n) => n.id === node.id)?.position) && <p>节点位置已变化</p>}<p className="whitespace-pre-wrap break-words">当前：{project?.nodes.find((n) => n.id === node.id)?.metadata?.content || "无正文"}</p><p className="whitespace-pre-wrap break-words">历史：{node.metadata?.content || "无正文"}</p></details>)}
                        <p className="my-3 text-xs">恢复会保留当前版本。历史任务不会重新提交；若素材已缺失，恢复仍保留引用，需另行找回原文件。</p>
                        <Button disabled={busy} onClick={() => void restore()}>恢复此版本</Button>
                    </> : <p>选择一个版本查看与当前画布的差异。</p>}
                </div>
            </div>
        </Modal>
    </>;
}
