import { describe, expect, test } from "bun:test";

import { desktopTaskIdFromStorageKey, materializeDesktopTaskMetadata } from "./canvas-local-task";

describe("desktop canvas task media keys", () => {
    test("accepts only one opaque local task id segment", () => {
        expect(desktopTaskIdFromStorageKey("local-task:task-123")).toBe("task-123");
        expect(desktopTaskIdFromStorageKey("server:task-123")).toBe(null);
        expect(desktopTaskIdFromStorageKey("local-task:")).toBe(null);
        expect(desktopTaskIdFromStorageKey("local-task:task-123:extra")).toBe(null);
    });

    test("materializes only the ephemeral playback URL", () => {
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
