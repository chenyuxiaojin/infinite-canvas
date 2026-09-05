import type { NextRequest } from "next/server";
import { forwardResponseBody, requestLifetime, RequestFailure } from "@/services/api/request-lifetime";

export const runtime = "nodejs";
export const maxDuration = 300;

type RouteContext = {
    params: Promise<{ path: string[] }>;
};

function proxyHeaders(request: NextRequest) {
    const headers = new Headers(request.headers);
    headers.delete("host");
    headers.delete("content-length");
    headers.delete("connection");
    headers.set("x-forwarded-host", request.nextUrl.host);
    headers.set("x-forwarded-proto", request.nextUrl.protocol.replace(":", ""));
    return headers;
}

function responseHeaders(response: Response) {
    const headers = new Headers(response.headers);
    headers.delete("content-length");
    headers.delete("content-encoding");
    headers.delete("transfer-encoding");
    return headers;
}

async function proxy(request: NextRequest, context: RouteContext) {
    const { path } = await context.params;
    const apiBaseUrl = process.env.API_BASE_URL || "http://127.0.0.1:8080";
    const target = `${apiBaseUrl.replace(/\/$/, "")}/api/${path.map(encodeURIComponent).join("/")}${request.nextUrl.search}`;
    const hasBody = request.method !== "GET" && request.method !== "HEAD";

    const incomingId = request.headers.get("x-request-id") || "";
    const requestId = /^[0-9a-f-]{36}$/i.test(incomingId) ? incomingId : crypto.randomUUID();
    // No total deadline: long streams remain alive while bytes arrive.
    const lifetime = requestLifetime(request.signal, 300_000);
    const headers = proxyHeaders(request);
    headers.set("x-request-id", requestId);
    try {
        const response = await fetch(target, {
            method: request.method,
            headers,
            signal: lifetime.signal,
            body: hasBody ? request.body : undefined,
            duplex: hasBody ? "half" : undefined,
            redirect: "manual",
        } as RequestInit & { duplex?: "half" });

        const outgoingHeaders = responseHeaders(response);
        outgoingHeaders.set("x-request-id", response.headers.get("x-request-id") || requestId);
        lifetime.touch();
        if (!response.body) lifetime.dispose();
        return new Response(response.body ? forwardResponseBody(response.body, lifetime) : null, {
            status: response.status,
            statusText: response.statusText,
            headers: outgoingHeaders,
        });
    } catch (error) {
        lifetime.dispose();
        const reason = lifetime.signal.reason;
        const kind = request.signal.aborted ? "cancelled" : reason instanceof RequestFailure ? reason.kind : "connect_failed";
        const msg = kind === "cancelled" ? "已停止等待" : kind === "read_timeout" ? "读取超时，服务长时间没有返回数据" : "接口连接失败，后端服务可能未启动或已退出";
        return Response.json({ code: 1, data: null, kind, requestId, submitted: hasBody, msg: msg + (hasBody ? "；请求可能已提交，请核对结果后再操作" : "") }, { status: kind === "cancelled" ? 499 : kind === "read_timeout" ? 504 : 502, headers: { "x-request-id": requestId } });
    }
}

export const GET = proxy;
export const HEAD = proxy;
export const POST = proxy;
export const PUT = proxy;
export const PATCH = proxy;
export const DELETE = proxy;
export const OPTIONS = proxy;
