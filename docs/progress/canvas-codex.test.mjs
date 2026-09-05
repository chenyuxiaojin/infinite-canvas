// Real frontend runtime with a local protocol double. No model calls or user data.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import test from "node:test";

const requireWeb = createRequire(new URL("../../web/package.json", import.meta.url));
const ts = requireWeb("typescript");
function load(path, imports) {
    const source = readFileSync(new URL("../../web/src/" + path, import.meta.url), "utf8");
    const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const module = { exports: {} };
    vm.runInNewContext(output, { module, exports: module.exports, console, DOMException, setTimeout, clearTimeout,
        require(name) { assert.ok(name in imports, `Unexpected import ${name}`); return imports[name]; },
    });
    return module.exports;
}
const trace = load("app/(user)/canvas/agent/canvas-context-trace.ts", {});
const tools = load("app/(user)/canvas/agent/canvas-agent-tools.ts", { nanoid: { nanoid: () => "action" } });
const apiRuntime = load("app/(user)/canvas/agent/canvas-agent-runtime.ts", {
    "@/services/api/canvas-agent": {}, "./canvas-agent-skills": {}, "./canvas-context-trace": trace, "./canvas-agent-tools": tools,
});
const usage = load("services/canvas-codex.ts", { "@tauri-apps/api/core": {} });

function harness(options = {}) {
    const calls = [], responses = [], updates = [], actions = [], texts = [];
    let listener, threadId = options.resume ? "existing-thread" : "new-thread", closed = 0;
    const emit = (method, params) => listener.onMessage({ method, params });
    const connection = {
        close() { closed++; }, notify: async (method) => calls.push({ method }),
        respond: async (id, result) => { responses.push({ id, result }); options.onResponse?.({ id, result, emit, listener }); },
        request: async (method, params) => {
            calls.push({ method, params });
            if (method === "account/read") return { accountType: options.account || "chatgpt" };
            if (method === "thread/start" || method === "thread/resume") return { thread: { id: threadId }, model: "configured-model" };
            if (method === "turn/start") queueMicrotask(() => options.onTurn ? options.onTurn({ emit, listener, threadId }) : emit("turn/completed", { threadId, turn: { status: "completed" } }));
            return {};
        },
    };
    const runtime = load("app/(user)/canvas/agent/canvas-codex-runtime.ts", {
        "@/services/canvas-codex": { openCanvasCodex: async (input) => { listener = input; return connection; } },
        "./canvas-agent-runtime": apiRuntime,
        "./canvas-agent-skills": { buildCanvasAgentSkillBundle: (_phase, _text, context) => ({ prompt: JSON.stringify(context), sources: [] }) },
        "./canvas-agent-tools": tools, "./canvas-context-trace": trace,
    });
    const context = { project: { id: "film", nodeCount: 2 }, agentState: apiRuntime.createCanvasAgentState(), selectedNodeIds: ["one"], nodes: [{ id: "one", title: "selected", type: "text", text: "first" }, { id: "two", title: "unrelated", text: "private large text" }], connections: [], tasks: [], generation: {} };
    const controller = new AbortController();
    const input = {
        projectId: "film", sessionId: "chat", threadId: options.resume ? threadId : undefined,
        config: {}, initialState: context.agentState, protocolMessages: [], userText: options.text || "读取这个节点", references: [],
        getContext: () => context, signal: controller.signal,
        onInvalidate: () => { controller.abort(); options.onInvalidate?.(); },
        executeAction: async (action) => { actions.push(action); return { ok: true, nodeId: "one" }; },
        onCodexUpdate: (patch) => updates.push(patch), onText: (text) => texts.push(text), onCheckpoint() {},
    };
    return { ...runtime, input, context, calls, actions, responses, updates, texts, controller, get closed() { return closed; } };
}

test("only relevant nodes enter the initial context", () => {
    const h = harness();
    const compact = h.compactCodexCanvasContext(h.context);
    assert.equal(compact.nodes.length, 1);
    assert.equal(compact.nodes[0].id, "one");
    assert.equal(h.context.nodes.length, 2);
});

test("context percentage does not guess a missing model limit", () => {
    assert.equal(usage.codexContextPercent(), null);
    assert.equal(usage.codexContextPercent({ inputTokens: 10, outputTokens: 2, contextWindow: null }), null);
    assert.equal(usage.codexContextPercent({ inputTokens: 60, outputTokens: 10, contextWindow: 100 }), 70);
});

test("official dynamic tool schema, actual model label, and native history resume", async () => {
    for (const resume of [false, true]) {
        const h = harness({ resume });
        await h.runCanvasCodex(h.input);
        const thread = h.calls.find((call) => call.method === (resume ? "thread/resume" : "thread/start"));
        assert.ok(thread);
        if (!resume) { assert.equal(thread.params.dynamicTools[0].type, "function"); assert.ok(thread.params.dynamicTools[0].inputSchema); }
        assert.ok(!thread.params.developerInstructions.includes("private large text"));
        assert.equal(h.updates[0].codexModel, "configured-model");
        assert.equal(h.closed, 1);
    }
});

test("API-key login cannot start a model request", async () => {
    const h = harness({ account: "apiKey" });
    await assert.rejects(h.runCanvasCodex(h.input), /ChatGPT/);
    assert.ok(!h.calls.some((call) => call.method === "turn/start"));
    assert.equal(h.closed, 1);
});

test("streamed answer, recent usage instead of cumulative totals, compaction event", async () => {
    const h = harness({ onTurn({ emit, threadId }) {
        emit("item/agentMessage/delta", { threadId, itemId: "answer", delta: "收到" });
        emit("thread/tokenUsage/updated", { threadId, tokenUsage: { last: { inputTokens: 20, outputTokens: 5 }, total: { inputTokens: 99999 }, modelContextWindow: 100 } });
        emit("item/completed", { threadId, item: { type: "contextCompaction" } });
        emit("item/completed", { threadId, item: { id: "answer", type: "agentMessage", phase: "final_answer", text: "读取完成" } });
        emit("turn/completed", { threadId, turn: { status: "completed" } });
    } });
    const result = await h.runCanvasCodex(h.input);
    assert.equal(result.reply, "读取完成");
    assert.ok(h.texts.includes("读取完成"));
    assert.equal(h.updates.find((update) => update.codexUsage).codexUsage.inputTokens, 20);
    assert.match(h.updates.find((update) => update.codexCompaction).codexCompaction, /已压缩/);
});

test("canvas tool executes once and returns real result through protocol", async () => {
    const h = harness({
        onTurn({ listener, threadId }) { listener.onMessage({ id: 99, method: "item/tool/call", params: { threadId, callId: "call", tool: "get_node", arguments: { nodeId: "one" } } }); },
        onResponse({ result, emit }) { assert.equal(result.contentItems[0].type, "inputText"); emit("turn/completed", { turn: { status: "completed" } }); },
    });
    await h.runCanvasCodex(h.input);
    assert.equal(h.actions.length, 1);
    assert.equal(JSON.parse(h.responses[0].result.contentItems[0].text).nodeId, "one");
});

test("unrequested arrangement cannot move nodes", async () => {
    const h = harness({
        onTurn({ listener, threadId }) { listener.onMessage({ id: 99, method: "item/tool/call", params: { threadId, tool: "arrange_nodes", arguments: {} } }); },
        onResponse({ emit }) { emit("turn/completed", { turn: { status: "completed" } }); },
    });
    await h.runCanvasCodex(h.input);
    assert.equal(h.actions.length, 0);
    assert.equal(h.responses[0].result.success, false);
});

test("model failure is not shown as success or automatically retried", async () => {
    const h = harness({ onTurn({ emit }) { emit("turn/completed", { turn: { status: "failed", error: { message: "quota exhausted" } } }); } });
    await assert.rejects(h.runCanvasCodex(h.input), /quota exhausted/);
    assert.equal(h.calls.filter((call) => call.method === "turn/start").length, 1);
    assert.equal(h.closed, 1);
});

test("repeated tool call IDs replay results without repeating canvas mutations", async () => {
    let count = 0;
    const request = { id: 99, method: "item/tool/call", params: { threadId: "new-thread", callId: "same-call", tool: "create_text_node", arguments: { title: "test", content: "text" } } };
    const h = harness({
        onTurn({ listener }) { listener.onMessage(request); },
        onResponse({ emit, listener }) {
            if (++count === 1) listener.onMessage({ ...request, id: 100 });
            else emit("turn/completed", { turn: { status: "completed" } });
        },
    });
    await h.runCanvasCodex(h.input);
    assert.equal(h.actions.length, 1);
    assert.equal(h.responses.length, 2);
});

test("a tool request for another thread cannot access this canvas", async () => {
    const h = harness({
        onTurn({ listener }) { listener.onMessage({ id: 99, method: "item/tool/call", params: { threadId: "another", tool: "delete_node", arguments: { nodeId: "one" } } }); },
        onResponse({ emit }) { emit("turn/completed", { turn: { status: "completed" } }); },
    });
    await h.runCanvasCodex(h.input);
    assert.equal(h.actions.length, 0);
    assert.equal(h.responses[0].result.success, false);
});

test("cancelled or interrupted turn is not reported as successful", async () => {
    const h = harness({ onTurn({ emit }) { emit("turn/completed", { turn: { status: "interrupted" } }); } });
    await assert.rejects(h.runCanvasCodex(h.input), (error) => error.name === "AbortError");
    assert.equal(h.closed, 1);
});

test("transport failure terminates the pending turn and closes its connection", async () => {
    const h = harness({ onTurn({ listener }) { listener.onClose(new Error("process exited")); } });
    await assert.rejects(h.runCanvasCodex(h.input), /process exited/);
    assert.equal(h.closed, 1);
    assert.equal(h.controller.signal.aborted, true);
});

test("disconnect invalidates an outstanding confirmation before it can perform a write", async () => {
    let listener, releaseConfirmation, writes = 0;
    const h = harness({
        onTurn(event) {
            listener = event.listener;
            listener.onMessage({ id: 99, method: "item/tool/call", params: { threadId: event.threadId, callId: "pending", tool: "delete_node", arguments: { nodeId: "one" } } });
        },
        onInvalidate() { releaseConfirmation?.(true); },
    });
    h.input.executeAction = async () => {
        const confirmed = await new Promise((resolve) => {
            releaseConfirmation = resolve;
            queueMicrotask(() => listener.onClose(new Error("disconnected while waiting")));
        });
        if (h.controller.signal.aborted) throw new DOMException("stopped", "AbortError");
        if (confirmed) writes++;
        return { ok: true };
    };
    await assert.rejects(h.runCanvasCodex(h.input), /disconnected while waiting/);
    assert.equal(writes, 0);
    assert.equal(h.closed, 1);
});

test("abort before a queued tool executes performs no canvas write", async () => {
    const h = harness({
        onTurn({ listener, threadId }) {
            h.controller.abort();
            listener.onMessage({ id: 99, method: "item/tool/call", params: { threadId, callId: "call", tool: "create_text_node", arguments: { title: "test", content: "text" } } });
        },
    });
    await assert.rejects(h.runCanvasCodex(h.input), (error) => error.name === "AbortError");
    assert.equal(h.actions.length, 0);
});

test("actual input trace appears only after the turn transport accepts it", async () => {
    const h = harness();
    const traces = [];
    h.input.onContextTrace = trace => traces.push(trace);
    await h.runCanvasCodex(h.input);
    assert.equal(traces.length, 1);
    assert.equal(traces[0].kind, "input");
    assert.deepEqual([...traces[0].nodes.map(node => node.id)], ["one"]);
    const failed = harness({account: "apiKey"});
    const failedTraces = [];
    failed.input.onContextTrace = trace => failedTraces.push(trace);
    await assert.rejects(failed.runCanvasCodex(failed.input), /登录/);
    assert.equal(failedTraces.length, 0);
});

test("API rejects unconfirmed image models before silently converting to text", async () => {
    await assert.rejects(apiRuntime.runCanvasAgent({
        config: {textModel: "unknown-text-only"}, initialState: apiRuntime.createCanvasAgentState(),
        protocolMessages: [], userText: "看这张图", references: [{id: "one", title: "原图", dataUrl: "data:image/png;base64,aGVsbG8="}],
    }), /没有按纯文字发送/);
});
