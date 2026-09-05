/** Abort remains cancellation; ending a wait never claims a submitted mutation was undone. */
export type RequestFailureKind = "cancelled" | "connect_failed" | "read_timeout" | "service_exited";
export class RequestFailure extends Error {
    constructor(public kind: RequestFailureKind, message: string, public requestId?: string, public submitted = false) {
        super(message);
        this.name = kind === "cancelled" ? "AbortError" : "RequestFailure";
    }
}

/** The deadline measures inactivity, not total media or conversation duration. */
export function requestLifetime(parent: AbortSignal, idleMs: number) {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const abort = () => controller.abort(parent.reason);
    const touch = () => {
        clearTimeout(timer);
        if (!controller.signal.aborted && idleMs > 0) {
            timer = setTimeout(() => controller.abort(new RequestFailure("read_timeout", "读取超时：服务长时间没有返回数据")), idleMs);
        }
    };
    const dispose = () => { clearTimeout(timer); parent.removeEventListener("abort", abort); };
    if (parent.aborted) abort(); else parent.addEventListener("abort", abort, { once: true });
    controller.signal.addEventListener("abort", () => clearTimeout(timer), { once: true });
    touch();
    return { signal: controller.signal, touch, dispose, abort: (reason?: unknown) => controller.abort(reason) };
}

export function forwardResponseBody(body: ReadableStream<Uint8Array>, lifetime: ReturnType<typeof requestLifetime>) {
    const reader = body.getReader();
    let finished = false;
    const close = () => { if (!finished) { finished = true; lifetime.dispose(); } };
    const abort = () => { void reader.cancel(lifetime.signal.reason).catch(() => {}); };
    lifetime.signal.addEventListener("abort", abort, { once: true });
    return new ReadableStream<Uint8Array>({
        async pull(controller) {
            try {
                if (lifetime.signal.aborted) throw lifetime.signal.reason;
                const { done, value } = await reader.read();
                if (lifetime.signal.aborted) throw lifetime.signal.reason;
                if (done) { close(); lifetime.signal.removeEventListener("abort", abort); controller.close(); }
                else { lifetime.touch(); controller.enqueue(value); }
            } catch (error) { close(); lifetime.signal.removeEventListener("abort", abort); controller.error(lifetime.signal.aborted ? lifetime.signal.reason : error instanceof RequestFailure ? error : new RequestFailure("service_exited", "数据流意外中断，服务可能已退出")); }
        },
        async cancel(reason) {
            lifetime.abort(reason); close(); lifetime.signal.removeEventListener("abort", abort);
            await reader.cancel(reason).catch(() => {});
        },
    });
}
