import { useQuery } from "@tanstack/react-query";
import { isDesktopRuntime } from "@/services/desktop-runtime";
import { inspectCanvasProjectBindings } from "@/services/desktop-terminal";
import { useCanvasStore } from "../stores/use-canvas-store";

export function useCanvasBindings() {
    const projects = useCanvasStore((state) => state.projects);
    const ids = projects.map((project) => project.id).sort();
    return useQuery({
        queryKey: ["canvas-binding-status", ids.join(",")],
        queryFn: () => inspectCanvasProjectBindings(ids),
        enabled: isDesktopRuntime() && ids.length > 0,
        staleTime: 30_000,
    });
}
