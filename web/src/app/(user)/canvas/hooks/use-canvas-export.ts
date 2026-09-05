import { App } from "antd";
import { exportCanvasProjects } from "../utils/canvas-export";
import type { CanvasProject } from "../stores/use-canvas-store";

export function useCanvasExport() {
    const { message } = App.useApp();
    return async (projects: CanvasProject[], name?: string) => {
        const dismiss = message.loading("正在核对素材并导出画布…", 0);
        try {
            const result = await exportCanvasProjects(projects, name);
            if (result?.saved !== false) message.success("画布与素材已完整导出");
        } catch (error) {
            message.error(error instanceof Error ? error.message : "导出失败");
        } finally { dismiss(); }
    };
}
