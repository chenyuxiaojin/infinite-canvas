// Tests the actual display service and hook with injected IPC/URL/React doubles.
// No app, browser, network, database, model call or original media is touched.
// The hook harness models effect/ref/state lifecycles, not React DOM or WebKit.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import test from "node:test";
import ts from "../../web/node_modules/typescript/lib/typescript.js";

const servicePath = new URL("../../web/src/services/canvas-local-image.ts", import.meta.url);
const hookPath = new URL("../../web/src/app/(user)/canvas/hooks/use-canvas-image-source.ts", import.meta.url);
const pagePath = new URL("../../web/src/app/(user)/canvas/[id]/canvas-client-page.tsx", import.meta.url);
const nodePath = new URL("../../web/src/app/(user)/canvas/components/canvas-node.tsx", import.meta.url);
const key = (id = "a") => `local-ref:asset-${id}`;
const png = () => Uint8Array.from([137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3]).buffer;
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
    let resolve, reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    return { promise, resolve, reject };
}
function loadSource(url, imports, globals = {}) {
    const source = readFileSync(url, "utf8");
    const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const module = { exports: {} };
    vm.runInNewContext(output, {
        module, exports: module.exports, ArrayBuffer, Uint8Array, Blob, Error,
        queueMicrotask, ...globals,
        require(name) { assert.ok(Object.hasOwn(imports, name), `Unexpected import: ${name}`); return imports[name]; },
    }, { filename: fileURLToPath(url) });
    return module.exports;
}
function serviceHarness({ desktop = true } = {}) {
    const calls = [], created = [], revoked = [];
    let active = 0, peak = 0;
    const service = loadSource(servicePath, {
        "@tauri-apps/api/core": {
            isTauri: () => desktop,
            invoke(command, args) {
                const pending = deferred();
                active++;
                peak = Math.max(peak, active);
                calls.push({ command, args, ...pending });
                return pending.promise.finally(() => { active--; });
            },
        },
    }, {
        URL: {
            createObjectURL(blob) { const url = `blob:isolated-${created.length + 1}`; created.push({ url, blob }); return url; },
            revokeObjectURL(url) { revoked.push(url); },
        },
    });
    const acquire = (projectId = "project-a", storageKey = key()) => {
        const lease = service.acquireCanvasLocalImage(projectId, storageKey);
        void lease.url.catch(() => {});
        return lease;
    };
    return { service, acquire, calls, created, revoked, get peak() { return peak; } };
}

test("stable source lookup preserves metadata and does not invent a readable URL", () => {
    const { service } = serviceHarness();
    const metadata = Object.freeze({ content: key("content"), storageKey: key("registered"), localMedia: Object.freeze({ rootId: "agent-media", relativePath: "verified/image.png" }) });
    const before = JSON.stringify(metadata);
    assert.equal(service.localCanvasImageKey(metadata), key("registered"));
    assert.equal(service.localCanvasImageKey({ content: key() }), key());
    assert.equal(service.localCanvasImageKey({ content: "https://example.invalid/image.png", storageKey: "image:existing" }), "");
    assert.equal(service.hasCanvasImageSource({ storageKey: key() }), true);
    assert.equal(service.hasCanvasImageSource({ content: "blob:existing" }), true);
    assert.equal(service.hasCanvasImageSource(undefined), false);
    assert.equal(JSON.stringify(metadata), before);
});

test("same project/key shares one IPC and one URL until its final consumer releases", async () => {
    const h = serviceHarness();
    const node = h.acquire(), preview = h.acquire();
    assert.equal(node.url, preview.url);
    await tick();
    assert.equal(h.calls.length, 1);
    assert.equal(h.calls[0].command, "read_canvas_local_image");
    assert.equal(JSON.stringify(h.calls[0].args), JSON.stringify({ projectId: "project-a", storageKey: key() }));
    h.calls[0].resolve(png());
    const url = await node.url;
    assert.equal(await preview.url, url);
    assert.equal(h.created.length, 1);
    assert.equal(h.created[0].blob.type, "image/png");
    assert.deepEqual(new Uint8Array(await h.created[0].blob.arrayBuffer()), new Uint8Array(png()));
    node.release(); node.release();
    assert.deepEqual(h.revoked, []);
    preview.release(); preview.release();
    assert.deepEqual(h.revoked, [url]);
});

test("different project IDs cannot share the same local reference", async () => {
    const h = serviceHarness();
    const a = h.acquire("project-a"), b = h.acquire("project-b");
    await tick();
    assert.equal(h.calls.length, 2);
    assert.notEqual(a.url, b.url);
    h.calls.forEach((call) => call.resolve(png()));
    assert.notEqual(await a.url, await b.url);
    a.release(); b.release();
    assert.equal(h.revoked.length, 2);
});

test("32 demanded images never exceed two concurrent IPC reads", async () => {
    const h = serviceHarness();
    const leases = Array.from({ length: 32 }, (_, i) => h.acquire("project", key(i)));
    await tick();
    assert.equal(h.calls.length, 2);
    for (let i = 0; i < leases.length; i++) {
        assert.ok(h.calls[i], `queued image ${i} started after a free slot`);
        h.calls[i].resolve(png());
        await leases[i].url;
        leases[i].release();
        await tick();
        assert.ok(h.peak <= 2);
    }
    assert.equal(h.calls.length, 32);
    assert.equal(h.peak, 2);
    assert.equal(h.revoked.length, 32);
});

test("release before the first drain never calls IPC", async () => {
    const h = serviceHarness();
    const lease = h.acquire();
    lease.release();
    assert.equal(await lease.url, "");
    await tick();
    assert.equal(h.calls.length, 0);
    assert.equal(h.created.length, 0);
});

test("released queued work is skipped without consuming a later queue entry", async () => {
    const h = serviceHarness();
    const a = h.acquire("p", key(1)), b = h.acquire("p", key(2));
    const skipped = h.acquire("p", key(3)), next = h.acquire("p", key(4));
    await tick();
    assert.equal(h.calls.length, 2);
    skipped.release(); skipped.release();
    assert.equal(await skipped.url, "");
    h.calls[0].resolve(png());
    await a.url; await tick();
    assert.equal(h.calls.length, 3);
    assert.equal(h.calls[2].args.storageKey, key(4));
    h.calls[1].resolve(png()); h.calls[2].resolve(png());
    await Promise.all([b.url, next.url]);
    a.release(); b.release(); next.release();
});

test("an in-flight result with no consumers creates no Blob URL", async () => {
    const h = serviceHarness();
    const lease = h.acquire();
    await tick();
    lease.release();
    h.calls[0].resolve(png());
    assert.equal(await lease.url, "");
    assert.equal(h.created.length, 0);
    assert.equal(h.revoked.length, 0);
});

test("a new consumer may reuse the same still in-flight request after release", async () => {
    const h = serviceHarness();
    const original = h.acquire();
    await tick();
    original.release();
    const next = h.acquire();
    assert.equal(original.url, next.url);
    assert.equal(h.calls.length, 1);
    h.calls[0].resolve(png());
    assert.match(await next.url, /^blob:/);
    next.release();
    assert.equal(h.revoked.length, 1);
});

test("after final release a fresh lease cannot reuse a revoked URL", async () => {
    const h = serviceHarness();
    const first = h.acquire();
    await tick(); h.calls[0].resolve(png());
    const oldUrl = await first.url;
    first.release();
    const second = h.acquire();
    await tick();
    assert.equal(h.calls.length, 2);
    h.calls[1].resolve(png());
    assert.notEqual(await second.url, oldUrl);
    second.release();
    assert.equal(h.revoked.length, 2);
});

test("invalid input or a non-desktop runtime fails without IPC", async () => {
    const h = serviceHarness({ desktop: false });
    assert.throws(() => h.acquire("", key()), /缺少/);
    assert.throws(() => h.acquire("project", "/arbitrary/file.png"), /缺少/);
    const lease = h.acquire();
    await assert.rejects(lease.url, /桌面应用/);
    lease.release();
    assert.equal(h.calls.length, 0);
    assert.equal(h.created.length, 0);
});

test("IPC failure is shared, releases its slot, and is not retried automatically", async () => {
    const h = serviceHarness();
    const failed = h.acquire("p", key(1)), busy = h.acquire("p", key(2)), queued = h.acquire("p", key(3));
    await tick();
    h.calls[0].reject(new Error("isolated backend rejection"));
    await assert.rejects(failed.url, /isolated backend rejection/);
    await tick();
    assert.equal(h.calls.length, 3);
    const sharedFailure = h.acquire("p", key(1));
    await assert.rejects(sharedFailure.url, /isolated backend rejection/);
    await tick();
    assert.equal(h.calls.length, 3);
    h.calls[1].resolve(png()); h.calls[2].resolve(png());
    await Promise.all([busy.url, queued.url]);
    failed.release(); sharedFailure.release(); busy.release(); queued.release();
    const explicitRetry = h.acquire("p", key(1));
    await tick();
    assert.equal(h.calls.length, 4);
    h.calls[3].resolve(png()); await explicitRetry.url; explicitRetry.release();
});

test("invalid IPC payloads and unsupported magic create no URL", async (t) => {
    const cases = [
        ["typed array instead of binary ArrayBuffer", new Uint8Array(png()), /大小无效/],
        ["plain array", [137, 80, 78, 71], /大小无效/],
        ["empty", new ArrayBuffer(0), /大小无效/],
        ["over 64 MiB", new ArrayBuffer(64 * 1024 * 1024 + 1), /64 MiB/],
        ["HTML instead of image", new TextEncoder().encode("<script>no</script>").buffer, /不是支持/],
        ["truncated signature", Uint8Array.from([137, 80]).buffer, /不是支持/],
        ["RIFF but not WEBP", Uint8Array.from([82, 73, 70, 70, 1, 2, 3, 4, 65, 86, 73, 32]).buffer, /不是支持/],
    ];
    for (const [name, payload, message] of cases) await t.test(name, async () => {
        const h = serviceHarness(), lease = h.acquire();
        await tick(); h.calls[0].resolve(payload);
        await assert.rejects(lease.url, message);
        await tick();
        assert.equal(h.calls.length, 1);
        assert.equal(h.created.length, 0);
        lease.release();
    });
});

test("supported image signatures receive the matching MIME without altering bytes", async (t) => {
    const cases = [
        ["image/png", png()],
        ["image/jpeg", Uint8Array.from([255, 216, 255, 224]).buffer],
        ["image/gif", new TextEncoder().encode("GIF87a...").buffer],
        ["image/gif", new TextEncoder().encode("GIF89a...").buffer],
        ["image/webp", new TextEncoder().encode("RIFF1234WEBP...").buffer],
    ];
    for (const [mime, bytes] of cases) await t.test(mime, async () => {
        const h = serviceHarness(), lease = h.acquire();
        await tick(); h.calls[0].resolve(bytes); await lease.url;
        assert.equal(h.created[0].blob.type, mime);
        assert.deepEqual(new Uint8Array(await h.created[0].blob.arrayBuffer()), new Uint8Array(bytes));
        lease.release();
    });
});

function hookHarness() {
    const slots = [], observers = [], leases = [];
    let cursor = 0, pending = [], dirty = false, mounted = true, projectId = "project-a", input, output;
    let lateStateWrites = 0;
    const react = {
        useRef(initial) { const i = cursor++; slots[i] ||= { kind: "ref", value: { current: initial } }; return slots[i].value; },
        useState(initial) {
            const i = cursor++; slots[i] ||= { kind: "state", value: typeof initial === "function" ? initial() : initial };
            return [slots[i].value, (next) => {
                if (!mounted) lateStateWrites++;
                const value = typeof next === "function" ? next(slots[i].value) : next;
                if (!Object.is(value, slots[i].value)) { slots[i].value = value; dirty = true; }
            }];
        },
        useEffect(setup, deps) {
            const i = cursor++;
            const old = slots[i];
            if (!old || deps.some((value, j) => !Object.is(value, old.deps[j]))) {
                slots[i] = { kind: "effect", deps, cleanup: old?.cleanup };
                pending.push({ i, setup });
            }
        },
    };
    const helper = serviceHarness().service;
    const hook = loadSource(hookPath, {
        react,
        "@/services/canvas-media-lease": {},
        "next/navigation": { useParams: () => ({ id: projectId }) },
        "@/services/canvas-local-image": {
            localCanvasImageKey: helper.localCanvasImageKey,
            acquireCanvasLocalImage(project, storageKey) {
                const d = deferred();
                const lease = { project, storageKey, url: d.promise, ...d, releases: 0, release() { this.releases++; } };
                leases.push(lease);
                return lease;
            },
        },
    }, {
        IntersectionObserver: class {
            constructor(callback) { this.callback = callback; this.disconnected = false; observers.push(this); }
            observe(element) { this.element = element; }
            disconnect() { this.disconnected = true; }
            emit(isIntersecting) { assert.equal(this.disconnected, false); this.callback([{ isIntersecting }]); }
        },
    });
    function render(next = input) {
        input = next;
        let renders = 0;
        do {
            assert.ok(++renders < 12, "hook reached a stable render without an effect loop");
            cursor = 0; pending = []; dirty = false;
            output = hook.useCanvasImageSource(input.metadata, input.enabled, input.observe);
            output.ref.current ||= { isolatedElement: true };
            const effects = pending;
            for (const { i } of effects) slots[i].cleanup?.();
            for (const { i, setup } of effects) slots[i].cleanup = setup();
        } while (dirty);
        return output;
    }
    return {
        render, observers, leases,
        get output() { return output; },
        get lateStateWrites() { return lateStateWrites; },
        setProject(id) { projectId = id; },
        async flush() { await tick(); if (dirty) render(); return output; },
        unmount() { for (const slot of slots) if (slot.kind === "effect") slot.cleanup?.(); mounted = false; },
    };
}

test("hook: ordinary URLs stay ordinary and disabled local images never acquire", () => {
    const h = hookHarness();
    assert.equal(h.render({ metadata: { content: "https://example.invalid/a.png" }, enabled: true, observe: true }).src, "https://example.invalid/a.png");
    assert.equal(h.observers.length, 0);
    assert.equal(h.leases.length, 0);
    assert.equal(h.render({ metadata: { storageKey: key() }, enabled: false, observe: true }).src, undefined);
    assert.equal(h.observers.length, 0);
    assert.equal(h.leases.length, 0);
    h.unmount();
});

test("hook: actual intersection gates acquisition and leaving releases the image", async () => {
    const h = hookHarness();
    h.render({ metadata: { content: key(), storageKey: key() }, enabled: true, observe: true });
    assert.equal(h.leases.length, 0);
    assert.equal(h.observers.length, 1);
    assert.ok(h.observers[0].element);
    h.observers[0].emit(false); h.render();
    assert.equal(h.leases.length, 0);
    h.observers[0].emit(true); h.render();
    assert.equal(h.leases.length, 1);
    h.leases[0].resolve("blob:visible");
    assert.equal((await h.flush()).src, "blob:visible");
    h.observers[0].emit(false); h.render();
    assert.equal(h.output.src, undefined);
    assert.equal(h.leases[0].releases, 1);
    h.unmount();
    assert.equal(h.observers[0].disconnected, true);
});

test("hook: switching project or reference cannot display an earlier asynchronous result", async () => {
    const h = hookHarness();
    const frozen = Object.freeze({ content: key(), storageKey: key(), localMedia: Object.freeze({ rootId: "agent-media", relativePath: "image.png" }) });
    const before = JSON.stringify(frozen);
    h.render({ metadata: frozen, enabled: true, observe: false });
    h.setProject("project-b"); h.render();
    assert.equal(h.leases[0].releases, 1);
    assert.equal(h.leases[1].project, "project-b");
    h.leases[0].resolve("blob:wrong-project");
    assert.equal((await h.flush()).src, undefined);
    h.leases[1].resolve("blob:project-b");
    assert.equal((await h.flush()).src, "blob:project-b");
    h.render({ metadata: { storageKey: key("next") }, enabled: true, observe: false });
    assert.equal(h.output.src, undefined);
    assert.equal(h.leases[1].releases, 1);
    h.leases[2].resolve("blob:next");
    assert.equal((await h.flush()).src, "blob:next");
    assert.equal(JSON.stringify(frozen), before);
    h.unmount();
});

test("hook: disable/re-enable hides the former lease and unmount ignores a late result", async () => {
    const h = hookHarness();
    const input = { metadata: { storageKey: key() }, enabled: true, observe: false };
    h.render(input); h.leases[0].resolve("blob:first");
    assert.equal((await h.flush()).src, "blob:first");
    h.render({ ...input, enabled: false });
    assert.equal(h.output.src, undefined);
    assert.equal(h.leases[0].releases, 1);
    h.render(input);
    assert.equal(h.output.src, undefined);
    assert.equal(h.leases.length, 2);
    h.unmount();
    h.leases[1].resolve("blob:after-unmount");
    await tick();
    assert.equal(h.leases[1].releases, 1);
    assert.equal(h.lateStateWrites, 0);
});

test("hook: failure is visible but ordinary rerenders do not auto-retry", async () => {
    const h = hookHarness();
    h.render({ metadata: { storageKey: key() }, enabled: true, observe: false });
    h.leases[0].reject(new Error("isolated missing media"));
    assert.equal((await h.flush()).error, "isolated missing media");
    for (let i = 0; i < 5; i++) h.render();
    assert.equal(h.leases.length, 1);
    h.unmount();
});

test("hook: absent project ID fails closed without acquiring media", () => {
    const h = hookHarness();
    h.setProject("");
    const result = h.render({ metadata: { storageKey: key() }, enabled: true, observe: false });
    assert.equal(result.error, "缺少当前画布绑定");
    assert.equal(result.src, undefined);
    assert.equal(h.leases.length, 0);
    h.unmount();
});

test("source wiring keeps local references out of mutating hydration and resolves only active preview", () => {
    const page = readFileSync(pagePath, "utf8"), node = readFileSync(nodePath, "utf8");
    const ast = ts.createSourceFile("page.tsx", page, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    let prepare;
    const visit = (entry) => {
        if (ts.isFunctionDeclaration(entry) && entry.name?.text === "prepareCanvasNodes") prepare = entry.getText(ast);
        ts.forEachChild(entry, visit);
    };
    visit(ast);
    assert.ok(prepare);
    const fn = vm.runInNewContext(ts.transpileModule(`(${prepare})`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText);
    const original = Object.freeze({ id: "registered", type: "image", metadata: Object.freeze({ content: key(), storageKey: key(), localMedia: Object.freeze({ rootId: "agent-media", relativePath: "verified/a.png" }) }) });
    const before = JSON.stringify(original);
    assert.equal(fn([original])[0], original);
    assert.equal(JSON.stringify(original), before);
    assert.match(page, /if \(localCanvasImageKey\(node\.metadata\)\) return false;/);
    assert.match(page, /storageKey=\{activeItemNode\.metadata\?\.storageKey\}/);
    assert.match(page, /useCanvasImageSource\(\{ content, storageKey \}, true, false, !isVideo\)/);
    assert.match(node, /useCanvasImageSource\(node\.metadata, !mediaLite, true\)/);
    assert.match(node, /ref=\{source\.ref\}/);
    assert.match(node, /src=\{source\.src\}/);
    assert.match(node, /media \? media\(source\.src\)/);
});
