import { isDesktopRuntime } from "@/services/desktop-runtime";
import { useCanvasBindings } from "../hooks/use-canvas-bindings";

export function CanvasBindingLabel({ projectId }: { projectId: string }) {
    const { data, error } = useCanvasBindings();
    if (!isDesktopRuntime()) return null;
    const binding = data?.find((item) => item.projectId === projectId);
    const label = error ? "目录状态读取失败" : !binding ? "正在核对目录…" : binding.state === "bound" ? binding.directories[0] : binding.message;
    return <div className="mt-2 min-w-0 text-xs opacity-70" title={`${label}\n画布 ${projectId}`}>
        <div className="truncate">{label}</div>
        <div className="mt-1 truncate">画布 {projectId}</div>
    </div>;
}
