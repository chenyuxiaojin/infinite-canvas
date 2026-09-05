import { apiDelete, apiGet, apiPost, compactApiParams } from "@/services/api/request";

export type Prompt = {
    id: string;
    title: string;
    coverUrl: string;
    prompt: string;
    tags: string[];
    category: string;
    githubUrl: string;
    preview: string;
    createdAt: string;
    updatedAt: string;
    remote?: boolean;
    saved?: boolean;
};

export const ALL_PROMPTS_OPTION = "全部";

export type PromptCategory = {
    category: string;
    name: string;
    description: string;
    githubUrl: string;
    sourceType?: string;
    pathOrUrl?: string;
    remote: boolean;
    enabled: boolean;
    updatedAt: string;
};

export type PromptListResponse = {
    items: Prompt[];
    tags: string[];
    categories: string[];
    total: number;
};

export async function fetchPrompts({ keyword = "", tag = [], category = ALL_PROMPTS_OPTION, page, pageSize, favorites = false, signal }: { keyword?: string; tag?: string[]; category?: string; page?: number; pageSize?: number; favorites?: boolean; signal?: AbortSignal } = {}) {
    return apiGet<PromptListResponse>(
        "/api/prompts",
        compactApiParams({
            ...(keyword ? { keyword } : {}),
            ...(tag.length ? { tag } : {}),
            ...(category !== ALL_PROMPTS_OPTION ? { category } : {}),
            ...(page ? { page } : {}),
            ...(pageSize ? { pageSize } : {}),
            ...(favorites ? { favorites: "true" } : {}),
        }),
        undefined,
        signal,
    );
}

export async function fetchPromptCategories() {
    return apiGet<PromptCategory[]>("/api/prompt-categories");
}

export async function syncPrompts(category?: string) {
    if (category) {
        await apiPost<PromptCategory[]>(`/api/prompts/sync?category=${encodeURIComponent(category)}`);
        return [];
    }
    // Separate requests let each source report failure without losing successful updates.
    const sources = (await fetchPromptCategories()).filter((item) => item.enabled && (item.remote || item.sourceType));
    const results: { name: string; error?: string }[] = [];
    for (const source of sources) {
        try {
            await syncPrompts(source.category);
            results.push({ name: source.name });
        } catch (error) {
            results.push({ name: source.name, error: error instanceof Error ? error.message : "更新失败" });
        }
    }
    return results;
}

export function fetchPromptDetail(id: string, signal?: AbortSignal) {
    return apiGet<Prompt>(`/api/prompts/${encodeURIComponent(id)}`, undefined, undefined, signal);
}

export function favoritePrompt(item: Prompt) {
    return apiPost<boolean>("/api/prompt-favorites", item);
}

export function unfavoritePrompt(id: string) {
    return apiDelete<boolean>(`/api/prompt-favorites/${encodeURIComponent(id)}`);
}

export async function savePromptCategory(item: Partial<PromptCategory>) {
    return apiPost<PromptCategory>("/api/prompt-categories", item);
}

export function formatPromptDate(value: string) {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? "" : new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit" }).format(date);
}
