import { invoke } from "@tauri-apps/api/core";
import type { CanvasProject } from "@/app/(user)/canvas/stores/use-canvas-store";

export type CanvasVersion = { sequence: number; revision: string; createdAt: string; reason: string; restoredFrom: number | null; bytes: number };
export type CanvasVersionPreview = { sequence: number; baseRevision: string; changes: Array<{ field: string; label: string; added: number; removed: number; changed: number }>; project: CanvasProject };
export const listCanvasVersions = (projectId: string) => invoke<CanvasVersion[]>("desktop_canvas_history", { projectId });
export const previewCanvasVersion = (projectId: string, sequence: number) => invoke<CanvasVersionPreview>("desktop_canvas_history_preview", { projectId, sequence });
