import { apiDelete, apiGet, apiPost } from "@/services/api/request";
import type { AiConfig } from "@/stores/use-config-store";
import { toProviderPayload, type UserS3StorageProvider, type UserStorageProvider, type UserWebDAVStorageProvider } from "@/services/image-storage";

export type UserConfigPayload = {
    modelConfig?: Partial<AiConfig>;
    storageProvider?: {
        s3?: Partial<UserS3StorageProvider>;
        webdav?: Partial<UserWebDAVStorageProvider>;
    };
    imageHistory?: unknown;
    assetData?: unknown;
    syncCapabilities?: {
        userData?: boolean;
        workflows?: boolean;
        assets?: boolean;
    };
};

export type StorageCapacityResult = {
    bytes: number;
    limitBytes: number;
    overLimit: boolean;
    checkedAt: string;
    providerName: string;
};

const userConfigRequests = new Map<string, Promise<UserConfigPayload>>();

export async function fetchUserConfig(token: string) {
    const existing = userConfigRequests.get(token);
    if (existing) return existing;
    const request = apiGet<UserConfigPayload>("/api/v1/user-config", undefined, token).finally(() => {
        if (userConfigRequests.get(token) === request) userConfigRequests.delete(token);
    });
    userConfigRequests.set(token, request);
    return request;
}

export async function syncUserModelConfig(token: string, config: AiConfig) {
    return apiPost<UserConfigPayload>("/api/v1/user-config/model", { config }, token);
}

export type UserStorageProviders = {
    s3?: UserS3StorageProvider;
    webdav?: UserWebDAVStorageProvider;
};

export async function syncUserStorageProvider(token: string, provider: UserStorageProviders) {
    return apiPost<UserConfigPayload>("/api/v1/user-config/storage", {
        provider: {
            ...(provider.s3 ? { s3: toProviderPayload(provider.s3) } : {}),
            ...(provider.webdav ? { webdav: toProviderPayload(provider.webdav) } : {}),
        },
    }, token);
}

export async function measureUserStorageProvider(token: string, provider: UserStorageProvider) {
    return apiPost<StorageCapacityResult>("/api/v1/storage/measure", { provider: toProviderPayload(provider) }, token);
}

export async function fetchUserImageHistory<T>(token: string) {
    return apiGet<T>("/api/v1/user-data/image-history", undefined, token);
}

export async function syncUserImageHistory<T>(token: string, data: T) {
    return apiPost<T>("/api/v1/user-data/image-history", { data }, token);
}

export async function fetchUserAssetData<T>(token: string) {
    return apiGet<T>("/api/v1/user-data/assets", undefined, token);
}

export async function syncUserAssetData<T>(token: string, data: T) {
    return apiPost<T>("/api/v1/user-data/assets", { data }, token);
}

export type CreativeWorkflowRecord<T = unknown> = {
    id: string;
    ownerUserId?: string;
    scope: "private" | "public";
    name: string;
    category: string;
    description: string;
    data: T;
    createdAt: string;
    updatedAt: string;
    lastRunAt?: string;
    editable: boolean;
};

export async function fetchUserWorkflows<T>(token: string) {
    return apiGet<Array<CreativeWorkflowRecord<T>>>("/api/v1/workflows", undefined, token);
}

export async function saveUserWorkflow<T>(token: string, workflow: CreativeWorkflowRecord<T>) {
    return apiPost<CreativeWorkflowRecord<T>>("/api/v1/workflows", workflow, token);
}

export async function deleteUserWorkflow(token: string, id: string) {
    return apiDelete<boolean>(`/api/v1/workflows/${encodeURIComponent(id)}`, token);
}

export type WorkflowAgentDraftResponse<T = unknown> = {
    draft: T;
    warnings: string[];
    model: string;
};

export async function draftUserWorkflow<T>(
    token: string,
    payload: {
        prompt: string;
        scope: "private" | "public";
        model?: string;
        channelId?: string;
        channelMode?: "remote" | "local";
        protocol?: string;
        baseUrl?: string;
        apiKey?: string;
        references?: string[];
    },
) {
    return apiPost<WorkflowAgentDraftResponse<T>>("/api/v1/workflows/agent-draft", payload, token);
}
