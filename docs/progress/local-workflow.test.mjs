// Isolated behavior checks: actual frontend modules, in-memory storage/IPC/network doubles.
// Does not start the App, use real credentials, call models or touch production data.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import test from "node:test";

const requireWeb = createRequire(new URL("../../web/package.json", import.meta.url));
const ts = requireWeb("typescript");
const { create } = requireWeb("zustand");
const root = new URL("../../web/src/", import.meta.url);
const noRequest = () => { throw new Error("Unexpected account/network request"); };
const jsx = (type, props) => ({ type, props });
const react = { useMemo: (fn) => fn(), useRef: (current) => ({ current }) };
function load(path, imports = {}, globals = {}) {
    const source = readFileSync(new URL(path, root), "utf8");
    const output = ts.transpileModule(source, { compilerOptions: {
        module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX,
    } }).outputText;
    const module = { exports: {} };
    vm.runInNewContext(output, {
        module, exports: module.exports, Blob, URL, URLSearchParams, console, structuredClone,
        setTimeout, clearTimeout, fetch: noRequest, ...globals,
        require(name) {
            if (name === "../protocol/canvas-operation-protocol") return load("app/(user)/canvas/protocol/canvas-operation-protocol.ts");
            if (name === "react/jsx-runtime") return { jsx, jsxs: jsx, Fragment: "fragment" };
            assert.ok(Object.hasOwn(imports, name), `Unexpected import ${name} in ${path}`);
            return imports[name];
        },
    }, { filename: path });
    return module.exports;
}

test("a saved legacy account is neither hydrated nor erased; shared services see a ready local session", async () => {
    const old = new Map([["infinite-canvas-auth-token-v1", '{"state":{"token":"test-legacy-token"}}']]);
    let storageAccesses = 0;
    const localStorage = {
        getItem(key) { storageAccesses++; return old.get(key); },
        setItem(key, value) { storageAccesses++; old.set(key, value); },
        removeItem(key) { storageAccesses++; old.delete(key); },
    };
    const { useUserStore } = load("stores/use-user-store.ts", { zustand: { create } }, { localStorage });
    await useUserStore.getState().hydrateUser();
    useUserStore.getState().clearSession();
    assert.equal(useUserStore.getState().token, "");
    assert.equal(useUserStore.getState().user, null);
    assert.equal(useUserStore.getState().isReady, true);
    assert.equal(storageAccesses, 0);
    assert.equal(old.size, 1);
});

function configHarness() {
    let persistence;
    const config = load("stores/use-config-store.ts", {
        react, zustand: { create },
        "zustand/middleware": { persist: (initializer, options) => { persistence = options; return initializer; } },
        "@/services/api/request": { apiGet: noRequest },
    });
    return { ...config, get persistence() { return persistence; } };
}

test("legacy remote preference resolves to the user's own AI channel without changing saved credentials", () => {
    const h = configHarness();
    const saved = { config: { channelMode: "remote", syncStorageConfig: true, localChannels: [
        { id: "own-channel", protocol: "openai", name: "My API", baseUrl: "https://ai.example.invalid", apiKey: "fixture-key", models: ["gpt-image-2"] },
    ], imageChannelId: "own-channel", imageModel: "gpt-image-2" } };
    const before = JSON.stringify(saved);
    const merged = h.persistence.merge(saved, h.useConfigStore.getState());
    assert.equal(merged.config.channelMode, "local");
    assert.equal(merged.config.localChannels[0].apiKey, "fixture-key");
    assert.equal(merged.config.localChannels[0].baseUrl, "https://ai.example.invalid");
    assert.equal(merged.config.imageChannelId, "own-channel");
    assert.equal(merged.config.syncStorageConfig, true); // old preference kept, no longer acted upon
    assert.equal(JSON.stringify(saved), before);
});

test("startup only loads local public settings; it never starts account hydration or synchronization", async () => {
    const effects = [], calls = [];
    const state = { loadPublicSettings: async () => calls.push("settings"), updateConfig: noRequest, openConfigDialog: noRequest };
    const { ClientRootInit } = load("components/layout/client-root-init.tsx", {
        react: { ...react, useEffect: (fn) => effects.push(fn) },
        "@/stores/use-config-store": { useConfigStore: (select) => select(state) },
    }, { window: { location: { search: "" } } });
    assert.equal(ClientRootInit({ children: "local-content" }).props.children, "local-content");
    for (const effect of effects) await effect();
    assert.deepEqual(calls, ["settings"]);
});

for (const [path, name, destination] of [
    ["app/(user)/login/page.tsx", "LoginPage", "/"],
    ["app/(admin)/admin/layout.tsx", "AdminLayout", "/"],
    ["app/(user)/asset-library/page.tsx", "AssetLibraryPage", "/assets"],
]) {
    test(`${name} redirects without mounting account content`, () => {
        const redirects = [];
        const page = load(path, { "next/navigation": { redirect: (to) => redirects.push(to) } });
        page.default();
        assert.deepEqual(redirects, [destination]);
    });
}

test("imported image/video/audio bytes stay local even if old cloud settings exist", async () => {
    const saved = new Map();
    const storage = { createInstance: () => ({ setItem: async (key, value) => saved.set(key, value), getItem: async (key) => saved.get(key) }) };
    let id = 0;
    const imports = {
        localforage: { default: storage }, nanoid: { nanoid: () => String(++id) },
        "@/services/anonymous-storage": { uploadAnonymousStorageFile: noRequest },
        "@/services/api/request": { apiGet: noRequest },
        "@/stores/use-user-store": { useUserStore: { getState: () => ({ token: "", user: null }) } },
        "@/lib/image-utils": { readImageMeta: async () => ({ width: 8, height: 6, mimeType: "image/png" }) },
    };
    const globals = { URL: { createObjectURL: () => `blob:fixture-${id}`, revokeObjectURL: () => {} }, window: { addEventListener() {}, localStorage: { getItem: () => '{"enabled":true}' } } };
    imports["./stored-object-url-cache"] = load("services/stored-object-url-cache.ts", {}, globals);
    imports["@tauri-apps/api/core"] = { isTauri: () => true };
    const images = load("services/image-storage.ts", imports, globals);
    const files = load("services/file-storage.ts", { ...imports, "@/services/image-storage": images }, globals);
    const bytes = new Uint8Array([1, 2, 3, 4]);
    const input = new Blob([bytes], { type: "image/png" });
    const image = await images.uploadImage(input);
    const imageWithEmptyOptions = await images.uploadImage(input, {});
    assert.match(image.storageKey, /^image:/);
    assert.match(imageWithEmptyOptions.storageKey, /^image:/);
    for (const prefix of ["asset-video", "asset-audio"]) {
        const result = await files.uploadAssetMediaFile(new Blob([bytes]), prefix);
        assert.ok(result.storageKey.startsWith(prefix + ":"));
        assert.deepEqual(new Uint8Array(await saved.get(result.storageKey).arrayBuffer()), bytes);
    }
    assert.deepEqual(new Uint8Array(await saved.get(image.storageKey).arrayBuffer()), bytes);
    assert.equal(await images.resolveImageUrl(image.storageKey), image.url);
    assert.equal(await images.resolveImageUrl("server:legacy", "https://media.example.invalid/original.png"), "https://media.example.invalid/original.png");
});

test("desktop canvas still loads and saves through local IPC without waiting for an account", async () => {
    let persistence;
    const writes = [], local = new Map();
    const original = { id: "existing-film", title: "片子", nodes: [{ id: "original-node" }], connections: [{ id: "original-edge", fromNodeId:"original-node", toNodeId:"original-node" }], updatedAt: "2026-01-01" };
    const before = JSON.stringify(original);
    const { useCanvasStore } = load("app/(user)/canvas/stores/use-canvas-store.ts", {
        zustand: { create }, nanoid: { nanoid: () => "new-fixture" },
        "fast-deep-equal": { default: (a,b) => assert.deepEqual(JSON.parse(JSON.stringify(a)),JSON.parse(JSON.stringify(b))) === undefined },
        "../utils/canvas-graph": load("app/(user)/canvas/utils/canvas-graph.ts"),
        "zustand/middleware": { persist: (initializer, options) => { persistence = options; return initializer; } },
        "@/lib/localforage-storage": { localForageStorage: { getItem: async (key) => local.get(key), setItem: async (key, value) => local.set(key, value) } },
        "@/services/api/canvas-tasks": { listCanvasProjects: noRequest, saveCanvasProject: noRequest, syncCanvasProjects: noRequest },
        "@/services/api/user-config": { fetchUserConfig: noRequest },
        "@/stores/use-user-store": { useUserStore: { getState: () => ({ token: "" }) } },
        "@/services/desktop-runtime": { isDesktopRuntime: () => true, loadDesktopCanvasDeletedIds: async () => [], loadDesktopCanvasProjects: async () => [original], saveDesktopCanvasProject: async (project) => { writes.push(project); return project; } },
    });
    const loaded = await persistence.storage.getItem("infinite-canvas:canvas_store");
    assert.equal(loaded.state.projects[0].nodes[0].id, "original-node");
    assert.equal(loaded.state.projects[0].connections[0].id, "original-edge");
    await useCanvasStore.getState().syncWithRemote("stale-fixture-token", true);
    useCanvasStore.setState({ projects: loaded.state.projects });
    useCanvasStore.getState().renameProject("existing-film", "改名");
    await new Promise((resolve) => setTimeout(resolve, 450));
    assert.equal(writes.length, 1);
    assert.equal(writes[0].title, "改名");
    assert.equal(JSON.stringify(original), before);
});

function uiImports(path, overrides) {
    const source = ts.createSourceFile(path, readFileSync(new URL(path, root), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const imports = {};
    for (const statement of source.statements) {
        if (!ts.isImportDeclaration(statement)) continue;
        const name = statement.moduleSpecifier.text;
        imports[name] = new Proxy({}, { get: (_target, key) => {
            if (key === "__esModule") return true;
            if (name.startsWith("@/services/")) return noRequest;
            return () => false;
        } });
    }
    return Object.assign(imports, overrides);
}
function elements(tree) {
    if (!tree || typeof tree !== "object") return [];
    if (Array.isArray(tree)) return tree.flatMap(elements);
    return [tree, ...elements(tree.props?.children)];
}

test("settings show the user's API fields and completing settings performs no account/storage request", () => {
    const h = configHarness(), changes = [], notices = [];
    const state = { ...h.useConfigStore.getState(), isConfigOpen: true,
        updateConfig: (key, value) => changes.push([key, value]),
        setConfigDialogOpen: (open) => changes.push(["dialog", open]), clearPromptContinue: () => {},
    };
    const path = "components/layout/app-config-modal.tsx";
    const tree = load(path, uiImports(path, {
        react: { ...react, useState: (value) => [typeof value === "function" ? value() : value, () => {}] },
        antd: { Button: "Button", Modal: "Modal", Select: "Select", Switch: "Switch", Form: { Item: "Form.Item" }, Input: { Password: "Input.Password", TextArea: "Input.TextArea" }, App: { useApp: () => ({ message: { warning: (text) => notices.push(text), success: (text) => notices.push(text) } }) } },
        "@/stores/use-config-store": { ...h, useConfigStore: (select) => select(state), useEffectiveConfig: () => state.config },
    })).AppConfigModal();
    const labels = JSON.stringify(tree);
    assert.match(labels, /Base URL/);
    assert.match(labels, /API Key/);
    assert.doesNotMatch(labels, /登录|云端渠道|自动同步|用户 S3|WebDAV 存储/);
    const modal = elements(tree).find((item) => item.props?.footer);
    modal.props.footer.props.onClick();
    assert.equal(changes.length, 1);
    assert.deepEqual(changes[0], ["dialog", false]);
    assert.equal(notices.length, 1);
});

test("canvas actions retain shortcuts/configuration without an account menu or balance", () => {
    const path = "components/layout/user-status-actions.tsx";
    let shortcuts = 0;
    const tree = load(path, uiImports(path, {
        "@/lib/utils": { cn: (...values) => values.join(" ") },
        "@/lib/canvas-theme": { canvasThemes: { dark: { node: { text: "#fff" } } } },
        "@/stores/use-theme-store": { useThemeStore: (select) => select({ theme: "dark", setTheme: () => {} }) },
        "@/stores/use-config-store": { useConfigStore: (select) => select({ openConfigDialog: () => {} }) },
    })).UserStatusActions({ variant: "canvas", onOpenShortcuts: () => shortcuts++ });
    const buttons = elements(tree).filter((item) => item.type === "button");
    assert.ok(buttons.some((item) => item.props["aria-label"] === "配置"));
    buttons.find((item) => item.props["aria-label"] === "快捷键").props.onClick();
    assert.equal(shortcuts, 1);
    assert.doesNotMatch(JSON.stringify(tree), /登录|账户菜单|算力点|\/admin/);
});

test("old account video jobs are left unchanged without polling or converting them to failures", () => {
    const path = "app/(user)/video/page.tsx";
    const source = ts.createSourceFile(path, readFileSync(new URL(path, root), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    let initializer;
    function visit(node) {
        if (ts.isVariableDeclaration(node) && node.name.getText(source) === "pollPendingLogsOnce") initializer = node.initializer;
        ts.forEachChild(node, visit);
    }
    visit(source);
    assert.ok(initializer);
    const calls = [];
    const poll = vm.runInNewContext(ts.transpileModule(`(${initializer.getText(source)})`, {
        compilerOptions: { target: ts.ScriptTarget.ES2022 },
    }).outputText, {
        token: "", pollingLogIdsRef: { current: new Set() }, effectiveConfigRef: { current: {} },
        buildResumeVideoConfig: (_config, log) => log.config, videoLogTaskId: (log) => log.task.id,
        isAiConfigReady: () => true, isLocalClientVideoLog: () => false,
        pollPendingLogOnce: (log) => calls.push(log.id),
    });
    const old = [
        { id: "remote", status: "生成中", task: { id: "old-1" }, config: { channelMode: "remote" } },
        { id: "account-proxy", status: "生成中", task: { id: "old-2", user_channel_id: "account-channel" }, config: { channelMode: "local" } },
        { id: "own-api", status: "生成中", task: { id: "own-3" }, config: { channelMode: "local" } },
    ];
    const before = JSON.stringify(old);
    poll(old);
    assert.deepEqual(calls, ["own-api"]);
    assert.equal(JSON.stringify(old), before);
});
