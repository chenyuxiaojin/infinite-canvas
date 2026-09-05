import axios from "axios";
import { RequestFailure } from "./request-lifetime";

export type ApiParams = Record<string, string | string[] | number | number[] | undefined>;

type ApiResponse<T> = {
    code: number;
    data: T;
    msg: string;
};

export function compactApiParams(params: ApiParams) {
    return Object.fromEntries(Object.entries(params).filter(([, value]) => value !== "" && value !== undefined && (!Array.isArray(value) || value.length > 0))) as ApiParams;
}

export function serializeApiParams(params?: ApiParams) {
    const queryParams = new URLSearchParams();
    for (const [key, value] of Object.entries(params || {})) {
        if (value === undefined) continue;
        if (Array.isArray(value)) value.forEach((item) => queryParams.append(key, String(item)));
        else queryParams.set(key, String(value));
    }
    return queryParams;
}

export async function apiGet<T>(url: string, params?: ApiParams, token?: string, signal?: AbortSignal) {
    return apiRequest<T>({
        url,
        method: "GET",
        params: params || undefined,
        headers: token ? { Authorization: `Bearer ${token}` } : undefined,
        signal,
    });
}

export async function apiPost<T>(url: string, body?: unknown, token?: string, signal?: AbortSignal) {
    return apiRequest<T>({
        url,
        method: "POST",
        signal,
        data: body ?? {},
        headers: {
            "Content-Type": "application/json",
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
    });
}

export async function apiDelete<T>(url: string, token?: string, signal?: AbortSignal) {
    return apiRequest<T>({
        url,
        method: "DELETE",
        signal,
        headers: token ? { Authorization: `Bearer ${token}` } : undefined,
    });
}

async function apiRequest<T>(config: { url: string; method: "GET" | "POST" | "DELETE"; params?: ApiParams; data?: unknown; headers?: Record<string, string>; signal?: AbortSignal }) {
    let response;
    const requestId = crypto.randomUUID();
    try {
        response = await axios.request<ApiResponse<T>>({
            url: config.url,
            method: config.method,
            params: config.params,
            paramsSerializer: { serialize: (params) => serializeApiParams(params as ApiParams).toString() },
            data: config.data,
            headers: { ...config.headers, "x-request-id": requestId },
            signal: config.signal,
            validateStatus: () => true,
        });
    } catch (error) {
        const submitted = config.method !== "GET";
        if (config.signal?.aborted || axios.isCancel(error)) throw new RequestFailure("cancelled", submitted ? "已停止等待；请求可能已提交，请核对结果" : "已取消请求", requestId, submitted);
        throw new RequestFailure("connect_failed", submitted ? "连接中断；请求可能已提交，请核对结果后再操作" : "接口连接失败，请确认后端服务已启动", requestId, submitted);
    }

    const result = response.data;
    if (!result || typeof result !== "object") {
        throw new Error(response.status === 404 ? "接口不存在，请确认后端服务已启动" : "接口返回异常，请稍后重试");
    }

    const payload = result as ApiResponse<T>;
    if (response.status < 200 || response.status >= 300 || payload.code !== 0) {
        const failure = result as ApiResponse<T> & { kind?: "cancelled" | "connect_failed" | "read_timeout" | "service_exited"; requestId?: string; submitted?: boolean };
        if (failure.kind) throw new RequestFailure(failure.kind, payload.msg || "请求失败", failure.requestId || response.headers["x-request-id"] || requestId, failure.submitted);
        throw new Error(payload.msg || "请求失败");
    }

    return payload.data;
}
