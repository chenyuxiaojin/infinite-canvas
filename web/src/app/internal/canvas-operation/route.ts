import { NextResponse } from "next/server";

import {
    applyCanvasOperationBatch,
    type CanvasOperationBatch,
    type CanvasProtocolProject,
} from "@/app/(user)/canvas/protocol/canvas-operation-protocol";

// The internal request carries the complete desktop project so the canonical
// reducer can return one atomic project snapshot. The public Agent payload is
// still limited to 1 MiB by the Bridge.
const MAX_REQUEST_BYTES = 64 * 1024 * 1024;

type WorkerRequest = {
    project: CanvasProtocolProject;
    batch: CanvasOperationBatch;
    now: string;
};

export async function POST(request: Request) {
    const contentLength = Number(request.headers.get("content-length") || 0);
    if (contentLength > MAX_REQUEST_BYTES) {
        return NextResponse.json({ ok: false, error: "request_too_large" }, { status: 413 });
    }

    try {
        const raw = await request.text();
        if (!raw || new TextEncoder().encode(raw).byteLength > MAX_REQUEST_BYTES) {
            return NextResponse.json({ ok: false, error: "invalid_request_size" }, { status: 400 });
        }
        const input = JSON.parse(raw) as WorkerRequest;
        if (!input?.project || !input?.batch || typeof input.now !== "string") {
            return NextResponse.json({ ok: false, error: "invalid_request" }, { status: 400 });
        }
        const outcome = applyCanvasOperationBatch(input.project, input.batch, { now: () => input.now });
        return NextResponse.json({ ok: true, outcome });
    } catch {
        return NextResponse.json({ ok: false, error: "invalid_request" }, { status: 400 });
    }
}
