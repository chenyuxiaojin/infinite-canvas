"use client";

import { create } from "zustand";
import type { AuthUser } from "@/services/api/auth";

type UserStore = {
    token: string;
    user: AuthUser | null;
    isReady: boolean;
    isLoading: boolean;
    clearSession: () => void;
    hydrateUser: () => Promise<void>;
};

// 本地版不读取、覆盖或删除旧账号存储。共享服务只读取空会话，不再触发账号请求。
export const useUserStore = create<UserStore>(() => ({
    token: "",
    user: null,
    isReady: true,
    isLoading: false,
    clearSession: () => {},
    hydrateUser: async () => {},
}));
