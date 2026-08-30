import { describe, expect, it } from "vitest";

import { desktopTaskIdFromStorageKey, materializeDesktopTaskMetadata } from "./canvas-local-task";

describe("desktop canvas task media keys", () => {
    it("accepts only one opaque local task id segment", () => {
        expect(desktopTaskIdFromStorageKey("local-task:task-123")).toBe("task-123");
        expect(desktopTaskIdFromStorageKey("server:task-123")).toBeNull();
        expect(desktopTaskIdFromStorageKey("local-task:")).toBeNull();
        expect(desktopTaskIdFromStorageKey("local-task:task-123:extra")).toBeNull();
    });

    it("materializes only the ephemeral playback URL", () => {
        const metadata = {
            content: "local-task:task-123",
            storageKey: "local-task:task-123",
            status: "success" as const,
            naturalWidth: 1344,
            naturalHeight: 768,
            durationMs: 6583,
            localTaskSha256: "a".repeat(64),
        };
        const materialized = materializeDesktopTaskMetadata(metadata, "blob:playback-only");
        expect(materialized).toEqual({ ...metadata, content: "blob:playback-only" });
        expect(materialized.bytes).toBeUndefined();
    });
});
