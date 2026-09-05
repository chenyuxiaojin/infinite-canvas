// Actual current service, Tauri Channel and xterm; deterministic IPC peer only.
// No installed App, GUI, project, account, model or network is used.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import vm from "node:vm";
import test from "node:test";

const requireWeb = createRequire(new URL("../../web/package.json", import.meta.url));
const { Terminal } = requireWeb("@xterm/xterm");
const { Channel } = requireWeb("@tauri-apps/api/core");
const ts = requireWeb("typescript");
const source = readFileSync(new URL("../../web/src/services/desktop-terminal.ts", import.meta.url), "utf8");
const rust = readFileSync(new URL("../../desktop/src-tauri/src/terminal.rs", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
const callbacks = new Map();
let callbackId = 0;
globalThis.window = { __TAURI_INTERNALS__: {
    transformCallback(fn) { callbacks.set(++callbackId, fn); return callbackId; },
    unregisterCallback(id) { callbacks.delete(id); },
} };
const tick = () => new Promise(resolve => setImmediate(resolve));
function deferred() { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const binary = value => Uint8Array.from(typeof value === "string" ? Buffer.from(value) : value).buffer;

function harness({ data = (_, done) => done(), ack = () => Promise.resolve(), spawn = () => Promise.resolve(true), error, serviceJs = js } = {}) {
    const calls = [], failures = [];
    let output, receive, index = 0, exits = 0;
    const module = { exports: {} };
    const invoke = (name, args) => {
        calls.push({ name, args });
        if (name === "pty_spawn") { output = args.onOutput; receive = callbacks.get(output.id); return spawn(args); }
        if (name === "pty_ack") return ack(args);
        if (name === "pty_terminate") return Promise.resolve();
        throw new Error(`unexpected ${name}`);
    };
    vm.runInNewContext(serviceJs, { module, exports: module.exports, window, ArrayBuffer, Uint8Array, Error, queueMicrotask,
        require(name) { assert.equal(name, "@tauri-apps/api/core"); return { Channel, invoke, isTauri: () => true }; },
    });
    const service = module.exports;
    return {
        calls, failures, service,
        start(id = "test") { return service.spawnPty({ session_id: id }, data, () => { exits++; }, message => { failures.push(message); error?.(message); }); },
        send(bytes) { receive({ index: index++, message: bytes }); },
        indexed(i, bytes) { receive({ index: i, message: bytes }); },
        end() { receive({ index, end: true }); },
        get id() { return output.id; },
        get exits() { return exits; },
        get acks() { return calls.filter(call => call.name === "pty_ack"); },
        get terminations() { return calls.filter(call => call.name === "pty_terminate"); },
    };
}

test("protocol constants agree and drawer ACKs only xterm's consumption callback", () => {
    const drawer = readFileSync(new URL("../../web/src/app/(user)/canvas/components/canvas-terminal-drawer.tsx", import.meta.url), "utf8");
    assert.match(source, /OUTPUT_BUDGET = 256 \* 1024/);
    assert.match(rust, /OUTPUT_BUDGET: usize = 256 \* 1024/);
    assert.match(drawer, /term\.write\(data, consumed\)/);
    assert.match(drawer, /setSpawnError\(error\)/);
});

test("ACK is not sent before consumption; duplicate callbacks grant no duplicate credit", async () => {
    let consume;
    const h = harness({ data: (_, done) => { consume = done; } });
    await h.start();
    h.send(binary("中文😀"));
    await tick();
    assert.equal(h.acks.length, 0);
    consume(); consume();
    await tick();
    assert.equal(h.acks.length, 1);
    assert.equal(h.acks[0].args.consumed, Buffer.byteLength("中文😀"));
    h.send(new ArrayBuffer(0));
    await tick();
    assert.equal(h.exits, 1);
    assert.equal(callbacks.has(h.id), false);
});

test("consumption batches coalesce, keep one ACK in flight, and EOF waits for the latest success", async () => {
    const pending = [];
    const h = harness({ ack: () => { const d = deferred(); pending.push(d); return d.promise; } });
    await h.start();
    h.send(binary("a")); h.send(binary("b"));
    await tick();
    assert.equal(h.acks.length, 1);
    assert.equal(h.acks[0].args.consumed, 2);
    h.send(binary("c")); h.send(binary("d"));
    await tick();
    assert.equal(h.acks.length, 1, "in-flight ACK retains only latest consumed boundary");
    pending[0].resolve(); await tick();
    assert.equal(h.acks.length, 2);
    assert.equal(h.acks[1].args.consumed, 4);
    h.send(new ArrayBuffer(0)); await tick();
    assert.equal(h.exits, 0);
    pending[1].resolve(); await tick();
    assert.equal(h.exits, 1);
    assert.deepEqual(h.failures, []);
    assert.equal(callbacks.has(h.id), false);
});

test("actual xterm plus ordered Channel preserve every Unicode split and one EOF", async () => {
    const expected = "跨块中文😀🧪 重复重复 END";
    const bytes = Buffer.from(expected);
    for (let split = 1; split < bytes.length; split++) {
        const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true });
        const h = harness({ data: (data, consumed) => term.write(data, consumed) });
        await h.start();
        h.indexed(1, binary(bytes.subarray(split)));
        h.indexed(0, binary(bytes.subarray(0, split)));
        while (h.acks.at(-1)?.args.consumed !== bytes.length && !h.failures.length) await tick();
        h.indexed(2, new ArrayBuffer(0));
        await tick();
        assert.deepEqual(h.failures, []);
        assert.equal(h.exits, 1);
        assert.equal(term.buffer.active.getLine(0).translateToString(true), expected);
        assert.equal(callbacks.has(h.id), false);
        term.dispose();
    }
});

test("56 MiB through actual xterm/Channel stays within 256 KiB and preserves bytes", { timeout: 60000 }, async () => {
    const total = 56 * 1024 * 1024;
    const pattern = Buffer.from("中文😀 0123456789 abcdefghijklmnopqrstuvwxyz\r\n");
    const payload = Buffer.alloc(total);
    for (let offset = 0; offset < total; offset += pattern.length) pattern.copy(payload, offset);
    const tail = Buffer.from("\r\nBACKPRESSURE_中文😀_DONE");
    const all = Buffer.concat([payload, tail]);
    const expectedHash = createHash("sha256").update(all).digest("hex");
    const deliveredHash = createHash("sha256");
    const term = new Terminal({ cols: 80, rows: 24, scrollback: 100, allowProposedApi: true });
    let sent = 0, acked = 0, maxOutstanding = 0, maxXtermPending = 0, scheduled = false, eof = false;
    const finished = deferred();
    const schedule = () => {
        if (scheduled) return;
        scheduled = true;
        setImmediate(() => { scheduled = false; produce(); });
    };
    const h = harness({
        data(data, consumed) {
            deliveredHash.update(data);
            term.write(data, consumed);
            maxXtermPending = Math.max(maxXtermPending, term._core._writeBuffer._pendingData);
        },
        ack({ consumed }) {
            assert.ok(consumed > acked && consumed <= sent);
            acked = consumed;
            schedule();
            return Promise.resolve();
        },
        error(message) { finished.reject(new Error(message)); },
    });
    const produce = () => {
        while (sent < all.length && sent - acked < 256 * 1024) {
            const end = Math.min(sent + 16384, all.length, acked + 256 * 1024);
            const chunk = all.subarray(sent, end);
            sent = end; // Same reserve-before-send order as actual Rust OutputFlow.
            maxOutstanding = Math.max(maxOutstanding, sent - acked);
            h.send(binary(chunk));
        }
        if (sent === all.length && acked === sent && !eof) {
            eof = true;
            h.send(new ArrayBuffer(0)); h.end();
            setImmediate(() => finished.resolve());
        }
    };
    try {
        await h.start();
        produce();
        await finished.promise;
        assert.ok(all.length > 50_003_968);
        assert.equal(acked, all.length);
        assert.equal(deliveredHash.digest("hex"), expectedHash);
        assert.ok(maxOutstanding <= 256 * 1024);
        assert.ok(maxXtermPending <= 256 * 1024);
        assert.equal(h.exits, 1);
        assert.deepEqual(h.failures, []);
        assert.equal(callbacks.has(h.id), false);
        const lines = Array.from({ length: term.buffer.active.length }, (_, i) => term.buffer.active.getLine(i).translateToString(true));
        assert.equal(lines.filter(line => line === "BACKPRESSURE_中文😀_DONE").length, 1);
        console.log(JSON.stringify({ bytes: all.length, maxOutstanding, maxXtermPending, hash: expectedHash, eof: h.exits, nativeGui: false }));
    } finally { await h.service.terminatePty("test"); term.dispose(); }
});

test("credit can deliver a new packet before the ACK RPC promise resolves", async () => {
    let h, sentExtra = false;
    h = harness({ ack: () => {
        if (!sentExtra) { sentExtra = true; h.send(new ArrayBuffer(16384)); }
        return Promise.resolve();
    } });
    await h.start();
    for (let i = 0; i < 16; i++) h.send(new ArrayBuffer(16384));
    await tick();
    assert.deepEqual(h.failures, []);
    assert.equal(h.acks.length, 2);
    assert.equal(h.acks.at(-1).args.consumed, 17 * 16384);
    await h.service.terminatePty("test");
});

test("synchronous writer exception cannot escape or strand the Channel", async () => {
    let calls = 0;
    const h = harness({ data: () => { calls++; throw new Error("write data discarded, isolated"); }, error: () => { throw new Error("UI also throws"); } });
    await h.start();
    assert.doesNotThrow(() => h.send(binary("first")));
    assert.doesNotThrow(() => h.send(binary("later")));
    h.send(new ArrayBuffer(0)); h.end(); await tick();
    assert.equal(calls, 1);
    assert.match(h.failures[0], /write data discarded/);
    assert.equal(h.terminations.length, 1);
    assert.equal(h.acks.length, 0);
    assert.equal(h.exits, 0);
    assert.equal(callbacks.has(h.id), false);
});

test("rejected ACK is visible, terminates and prevents later acknowledgements", async () => {
    const h = harness({ ack: () => Promise.reject(new Error("isolated ACK failure")) });
    await h.start();
    h.send(binary("a")); h.send(binary("b"));
    await tick();
    assert.match(h.failures[0], /ACK failure/);
    assert.equal(h.acks.length, 1);
    assert.equal(h.terminations.length, 1);
    assert.equal(callbacks.has(h.id), false);
    assert.equal(h.exits, 0);
});

test("close during slow consumption deregisters callback and late writes cannot ACK", async () => {
    const pending = [];
    const h = harness({ data: (_, done) => pending.push(done) });
    await h.start();
    for (let i = 0; i < 16; i++) h.send(new ArrayBuffer(16384));
    assert.equal(h.acks.length, 0);
    await h.service.terminatePty("test");
    pending.forEach(done => done());
    await tick();
    assert.equal(h.acks.length, 0);
    assert.equal(callbacks.has(h.id), false);
    assert.deepEqual(h.failures, []);
});

test("late spawn after close is terminated again; duplicate active IDs are rejected", async () => {
    const pending = deferred();
    const h = harness({ spawn: () => pending.promise });
    const started = h.start();
    await assert.rejects(h.start(), /已存在/);
    await h.service.terminatePty("test");
    assert.equal(callbacks.has(h.id), false);
    await assert.rejects(h.start(), /已存在/, "ID remains reserved until the earlier spawn settles");
    pending.resolve(true);
    assert.equal(await started, true);
    assert.equal(h.terminations.length, 2);
});

test("failed invoke before first packet removes callback and reports the failure", async () => {
    const h = harness({ spawn: () => Promise.reject(new Error("spawn rejected")) });
    await assert.rejects(h.start(), /spawn rejected/);
    assert.equal(callbacks.has(h.id), false);
    assert.equal(h.terminations.length, 1);
    assert.match(h.failures[0], /spawn rejected/);
});

test("invalid output, early EOF and out-of-order consumption visibly fail closed", async t => {
    for (const scenario of ["invalid", "early EOF", "budget", "oversized chunk", "callback order", "backend error"]) await t.test(scenario, async () => {
        const pending = [];
        const h = harness({ data: (_, done) => pending.push(done) });
        await h.start();
        if (scenario === "invalid") h.send({ malformed: true });
        if (scenario === "backend error") h.send({ error: "backend read failed" });
        if (scenario === "early EOF") { h.send(binary("a")); h.send(new ArrayBuffer(0)); }
        if (scenario === "budget") for (let i = 0; i < 17; i++) h.send(new ArrayBuffer(16384));
        if (scenario === "oversized chunk") h.send(new ArrayBuffer(16385));
        if (scenario === "callback order") { h.send(binary("a")); h.send(binary("b")); pending[1](); }
        await tick();
        assert.equal(h.failures.length, 1);
        assert.equal(h.terminations.length, 1);
        assert.equal(h.exits, 0);
        assert.equal(callbacks.has(h.id), false);
    });
});

test("paused consumption stays bounded and resumes with one cumulative ACK", async () => {
    const pending = [];
    const h = harness({ data: (_, done) => pending.push(done) });
    await h.start();
    for (let i = 0; i < 256; i++) h.send(new ArrayBuffer(1024));
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(h.acks.length, 0);
    assert.equal(h.terminations.length, 0);
    pending.forEach(done => done());
    await tick();
    assert.equal(h.acks.length, 1);
    assert.equal(h.acks[0].args.consumed, 256 * 1024);
    h.send(new ArrayBuffer(0)); await tick();
    assert.equal(h.exits, 1);
    assert.equal(callbacks.has(h.id), false);
});

test("rejected last cumulative ACK cannot turn an already-arrived EOF into success", async () => {
    const ack = deferred();
    const h = harness({ ack: () => ack.promise });
    await h.start();
    h.send(binary("a")); h.send(binary("b"));
    await tick();
    assert.equal(h.acks[0].args.consumed, 2);
    h.send(new ArrayBuffer(0));
    ack.reject(new Error("last cumulative ACK failed"));
    await tick();
    assert.equal(h.exits, 0);
    assert.equal(h.terminations.length, 1);
    assert.match(h.failures[0], /last cumulative ACK failed/);
    assert.equal(callbacks.has(h.id), false);
});

test("synchronous ACK invocation failure is caught outside the Channel callback", async () => {
    const h = harness({ ack: () => { throw new Error("synchronous IPC failure"); } });
    await h.start();
    assert.doesNotThrow(() => h.send(binary("a")));
    await tick();
    assert.match(h.failures[0], /synchronous IPC failure/);
    assert.equal(h.terminations.length, 1);
    assert.equal(h.exits, 0);
    assert.equal(callbacks.has(h.id), false);
});

test("close during an ACK request suppresses the queued cumulative ACK", async () => {
    const ack = deferred();
    const h = harness({ ack: () => ack.promise });
    await h.start();
    h.send(binary("a")); await tick();
    h.send(binary("b"));
    await h.service.terminatePty("test");
    ack.resolve(); await tick();
    assert.equal(h.acks.length, 1);
    assert.equal(h.exits, 0);
    assert.equal(callbacks.has(h.id), false);
});

test("actual xterm and 1 KiB packets with asynchronous 2 ms ACK latency compare serial and cumulative", { timeout: 20000 }, async () => {
    const serialSource = readFileSync(new URL("./fixtures/terminal-serial-ack.ts", import.meta.url), "utf8");
    assert.match(serialSource, /acknowledgements = acknowledgements.then/);
    const serialJs = ts.transpileModule(serialSource, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const payload = Buffer.alloc(2 * 1024 * 1024).fill(Buffer.from("小包中文😀 0123456789\r\n"));
    const expectedHash = createHash("sha256").update(payload).digest("hex");
    const run = async serviceJs => {
        const term = new Terminal({ cols: 80, rows: 24, scrollback: 100, allowProposedApi: true });
        const hash = createHash("sha256");
        const finished = deferred();
        let sent = 0, acked = 0, maxOutstanding = 0, maxXtermPending = 0, inFlight = 0, maxInFlight = 0, scheduled = false, eof = false;
        const schedule = () => {
            if (scheduled) return;
            scheduled = true;
            setImmediate(() => { scheduled = false; produce(); });
        };
        const h = harness({ serviceJs,
            data(data, done) { hash.update(data); term.write(data, done); maxXtermPending = Math.max(maxXtermPending, term._core._writeBuffer._pendingData); },
            ack({ consumed }) {
                inFlight++; maxInFlight = Math.max(maxInFlight, inFlight);
                return new Promise(resolve => setTimeout(() => {
                    assert.ok(consumed > acked && consumed <= sent && consumed % 1024 === 0);
                    acked = consumed;
                    inFlight--;
                    schedule();
                    resolve();
                }, 2));
            },
            error(message) { finished.reject(new Error(message)); },
        });
        const produce = () => {
            while (sent < payload.length && sent - acked < 256 * 1024) {
                const end = Math.min(sent + 1024, payload.length, acked + 256 * 1024);
                const chunk = payload.subarray(sent, end);
                sent = end;
                maxOutstanding = Math.max(maxOutstanding, sent - acked);
                h.send(binary(chunk));
            }
            if (acked === payload.length && !eof) {
                eof = true;
                h.send(new ArrayBuffer(0)); h.end();
                setImmediate(() => finished.resolve());
            }
        };
        const started = performance.now();
        try {
            await h.start(); produce(); await finished.promise;
            assert.equal(hash.digest("hex"), expectedHash);
            assert.equal(h.exits, 1);
            assert.deepEqual(h.failures, []);
            assert.equal(callbacks.has(h.id), false);
            assert.equal(maxInFlight, 1);
            assert.ok(maxOutstanding <= 256 * 1024 && maxXtermPending <= 256 * 1024);
            return { bytes: acked, ackRequests: h.acks.length, wallMs: performance.now() - started, maxOutstanding, maxXtermPending, maxInFlight, hash: expectedHash };
        } finally { await h.service.terminatePty("test"); term.dispose(); }
    };
    const serial = await run(serialJs);
    const cumulative = await run(js);
    assert.equal(serial.ackRequests, payload.length / 1024);
    assert.ok(cumulative.ackRequests < serial.ackRequests / 8);
    console.log(JSON.stringify({ benchmark: "actual xterm and Channel; deterministic IPC with real asynchronous timers; NOT native WebKit", packetBytes: 1024, ackDelayMs: 2, serial, cumulative }));
});

test("all test Channel callbacks are released", () => assert.equal(callbacks.size, 0));
