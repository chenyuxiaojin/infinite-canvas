import type { CanvasProject } from "./stores/use-canvas-store";
import type { LocalMediaReference } from "./types";

export type CanvasExportFile = {
    app: "infinite-canvas";
    version: 3 | 4 | 5;
    exportedAt: string;
    mediaMode?: "embedded" | "references";
    projects: CanvasProjectExportItem[];
};

export type CanvasProjectExportItem = {
    project: CanvasProject;
    files: CanvasExportAsset[];
};

export type CanvasExportAsset = {
    storageKey: string;
    path: string;
    mimeType: string;
    bytes: number;
    embedded?: boolean;
    reference?: LocalMediaReference;
};
