// Independent deterministic audit; no App IPC, browser UI, real project, or model calls.
// Run: node --test docs/progress/terminal-audit.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import test from "node:test";

const repo = fileURLToPath(new URL("../../", import.meta.url));
const requireWeb = createRequire(`${repo}web/package.json`);
const { Terminal } = requireWeb("@xterm/xterm");
const { Channel } = requireWeb("@tauri-apps/api/core");
const ts = requireWeb("typescript");
const callbacks = new Map();
let callbackId = 0;
globalThis.window = {
    __TAURI_INTERNALS__: {
        transformCallback(callback) { callbacks.set(++callbackId, callback); return callbackId; },
        unregisterCallback(id) { callbacks.delete(id); },
    },
};

function loadService(invoke) {
    // Frozen pre-backpressure service for historical comparison. Current
    // protocol coverage lives in terminal-backpressure.test.mjs.
    const source = `import { Channel, invoke } from "@tauri-apps/api/core";
    export function spawnPty(options, onData, onExit) {
        const onOutput = new Channel((buffer) => {
            if (!buffer.byteLength) onExit(); else onData(new Uint8Array(buffer));
        });
        return invoke("pty_spawn", { options, onOutput });
    }`;
    const js = ts.transpileModule(source, {
        compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const module = { exports: {} };
    vm.runInNewContext(js, {
        exports: module.exports, module, Uint8Array,
        require(name) {
            assert.equal(name, "@tauri-apps/api/core");
            return { Channel, invoke, isTauri: () => true };
        },
    });
    return module.exports;
}

function write(term, data) {
    return new Promise((resolve) => term.write(data, resolve));
}

function contents(term) {
    return Array.from({ length: term.buffer.active.length }, (_, index) =>
        term.buffer.active.getLine(index).translateToString(true));
}

test("actual xterm byte input preserves Chinese and emoji at every split", async () => {
    const expected = "中文😀🧪｜重复重复｜EOF";
    const bytes = new Uint8Array(Buffer.from(expected));
    let oldAlgorithmFailures = 0;
    for (let split = 1; split < bytes.length; split++) {
        const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
        await write(term, bytes.slice(0, split));
        await write(term, bytes.slice(split));
        assert.equal(contents(term)[0], expected, `split at byte ${split}`);
        const oldText = Buffer.from(bytes.slice(0, split)).toString("utf8")
            + Buffer.from(bytes.slice(split)).toString("utf8");
        if (oldText !== expected) oldAlgorithmFailures++;
        term.dispose();
    }
    console.log(JSON.stringify({ splits: bytes.length - 1, currentFailures: 0, oldPerChunkDecoderModelFailures: oldAlgorithmFailures }));
});

test("historical pre-backpressure service and actual Channel order binary chunks and EOF once", async () => {
    let output;
    const service = loadService((name, args) => {
        assert.equal(name, "pty_spawn");
        assert.equal(args.options.session_id, "audit-in-memory");
        output = args.onOutput;
        return Promise.resolve(true);
    });
    const term = new Terminal({ cols: 80, rows: 24, scrollback: 5000, allowProposedApi: true });
    let eof = 0;
    const events = [];
    assert.equal(await service.spawnPty({ session_id: "audit-in-memory" }, (bytes) => {
        assert.ok(bytes instanceof Uint8Array);
        events.push("data");
        term.write(bytes);
    }, () => { events.push("eof"); eof++; }), true);
    const expected = Array.from({ length: 1000 }, (_, i) => `${String(i).padStart(4, "0")} 中文😀 重复重复`);
    const bytes = new Uint8Array(Buffer.from(`${expected.join("\r\n")}\r\n`));
    const chunks = [];
    for (let start = 0; start < bytes.length; start += 17) chunks.push(bytes.slice(start, start + 17));
    const deliver = callbacks.get(output.id);
    // Deliberately deliver all chunks backwards, including EOF first.
    deliver({ index: chunks.length, message: new ArrayBuffer(0) });
    for (let i = chunks.length - 1; i >= 0; i--) deliver({ index: i, message: chunks[i].buffer });
    deliver({ index: chunks.length + 1, end: true });
    await write(term, "");
    assert.deepEqual(contents(term).slice(0, 1000), expected);
    assert.equal(eof, 1);
    assert.equal(events.at(-1), "eof");
    assert.equal(callbacks.has(output.id), false);
    console.log(JSON.stringify({ lines: expected.length, bytes: bytes.length, chunks: chunks.length, duplicatedOrMissingLines: 0, eof }));
    term.dispose();
});

test("headless xterm finite 8 MiB parse benchmark, not App rendering acceptance", async () => {
    const term = new Terminal({ cols: 80, rows: 24, scrollback: 5000, allowProposedApi: true });
    const chunk = new Uint8Array(Buffer.from("audit 中文😀 0123456789 abcdefghijklmnopqrstuvwxyz\r\n".repeat(300)));
    const iterations = Math.ceil(8 * 1024 * 1024 / chunk.byteLength);
    let maxHeartbeatDelayMs = 0;
    let heartbeats = 0;
    let lastHeartbeat = performance.now();
    const heartbeat = setInterval(() => {
        const now = performance.now();
        maxHeartbeatDelayMs = Math.max(maxHeartbeatDelayMs, now - lastHeartbeat - 16);
        lastHeartbeat = now;
        heartbeats++;
    }, 16);
    const cpu = process.cpuUsage();
    const start = performance.now();
    for (let i = 0; i < iterations; i++) term.write(chunk);
    await write(term, "AUDIT_FINAL_MARKER");
    clearInterval(heartbeat);
    const wallMs = performance.now() - start;
    const cpuUsed = process.cpuUsage(cpu);
    assert.equal(contents(term).filter(line => line === "AUDIT_FINAL_MARKER").length, 1);
    console.log(JSON.stringify({ benchmark: "Node/no-DOM/no-WebView/no-PTY", bytes: iterations * chunk.byteLength, wallMs, cpuUserMs: cpuUsed.user / 1000, cpuSystemMs: cpuUsed.system / 1000, rssBytes: process.memoryUsage().rss, heartbeats, maxHeartbeatDelayMs }));
    term.dispose();
});

test("reproduce overload failure: xterm throw stalls subsequent ordered Channel data and EOF", async () => {
    const term = new Terminal({ cols: 80, rows: 24, scrollback: 0 });
    let eof = 0;
    let delivered = 0;
    const channel = new Channel((buffer) => {
        if (!buffer.byteLength) eof++;
        else { term.write(new Uint8Array(buffer)); delivered++; }
    });
    const receive = callbacks.get(channel.id);
    const chunk = new Uint8Array(16384).fill(120);
    let failure;
    let failedIndex;
    // Synchronous arrivals deliberately exceed the actual xterm 50 MB queue cap.
    for (let index = 0; index < 4000; index++) {
        try { receive({ index, message: chunk.buffer }); }
        catch (error) { failure = error; failedIndex = index; break; }
    }
    assert.match(failure?.message ?? "", /write data discarded/);
    const pendingAtFailure = term._core._writeBuffer._pendingData;
    const before = delivered;
    receive({ index: failedIndex + 1, message: new Uint8Array([65]).buffer });
    receive({ index: failedIndex + 2, message: new ArrayBuffer(0) });
    receive({ index: failedIndex + 3, end: true });
    assert.equal(delivered, before);
    assert.equal(eof, 0);
    assert.equal(callbacks.has(channel.id), true);
    // Test cleanup only: use xterm's internal no-op parser to drain the queued
    // artificial flood quickly. Failure above used the real unmodified writer.
    term._core._writeBuffer._action = () => undefined;
    while (term._core._writeBuffer._pendingData) await new Promise(resolve => setTimeout(resolve, 1));
    receive({ index: failedIndex + 4, message: new Uint8Array([66]).buffer });
    assert.equal(delivered, before, "Channel remains stalled even after xterm drains");
    console.log(JSON.stringify({ reproducedProductRisk: true, exception: failure.message, failedIndex, pendingBytes: pendingAtFailure, deliveredBeforeFailure: delivered, eofDelivered: eof, callbackRetained: callbacks.has(channel.id) }));
    channel.cleanupCallback();
    term.dispose();
});
