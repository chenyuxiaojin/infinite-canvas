// Real frontend runtimes with protocol doubles. No CLI, model request or real canvas writes.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import test from 'node:test';
const requireWeb = createRequire(new URL('../../web/package.json', import.meta.url));
const ts = requireWeb('typescript');
function load(path, imports) {
    const source = readFileSync(new URL('../../web/src/' + path, import.meta.url), 'utf8');
    const output = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
    const module = { exports: {} };
    vm.runInNewContext(output, { module, exports: module.exports, DOMException, setTimeout, clearTimeout, console,
        require(name) { assert.ok(name in imports, name); return imports[name]; } });
    return module.exports;
}
const trace = load("app/(user)/canvas/agent/canvas-context-trace.ts", {});
const tools = load('app/(user)/canvas/agent/canvas-agent-tools.ts', { nanoid: { nanoid: () => 'action' } });
const api = load('app/(user)/canvas/agent/canvas-agent-runtime.ts', { '@/services/api/canvas-agent': {}, './canvas-agent-skills': {}, './canvas-context-trace': trace, './canvas-agent-tools': tools });
function harness(provider = 'grok', options = {}) {
    const calls = [], actions = [], responses = [], texts = [], ids = [], grants = [], models = [];
    let listener, closed = 0;
    const controller = new AbortController();
    const emit = (message) => listener.onMessage(message);
    const connection = {
        close() { closed++; },
        async request(method, params) {
            calls.push({ method, params });
            if (method === 'initialize') return { authMethods: [{ id: 'cached_token' }], agentCapabilities: { loadSession: true } };
            if (method === 'session/new') return { sessionId: 'owned', models: {currentModelId:'grok-4.6'} };
            if (method === 'session/prompt') {
                await options.onTurn?.({ emit, listener, controller });
                if (!options.onTurn) emit({ method: 'session/update', params: { sessionId: 'owned', update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: '你好' } } } });
                return { stopReason: options.stopReason || 'end_turn' };
            }
            return {};
        },
        async send(message) {
            calls.push(message);
            if (options.onTurn) await options.onTurn({ emit, listener, controller });
            else {
                emit({ event: 'step_update', step_update: { step_type: 'agent_response', text_delta: '草稿' } });
                emit({ event: 'result', result: { status: 'SUCCESS', response: '最终答案' } });
            }
        },
        async respond(id, result) { responses.push({ id, result }); options.onResponse?.({ emit, result }); },
        async permission(id, option) { grants.push({ id, option }); },
    };
    const runtime = load('app/(user)/canvas/agent/canvas-local-agent-runtime.ts', {
        '@/services/canvas-local-agent': { openCanvasLocalAgent: async (input) => { listener = input; if (provider === 'antigravity') { if (options.initError) input.onClose(new Error(options.initError)); else emit({event:'init',conversation_id:'owned'}); } return connection; } },
        './canvas-agent-runtime': api, './canvas-codex-runtime': { compactCodexCanvasContext: (context) => context },
        './canvas-agent-skills': { buildCanvasAgentSkillBundle: () => ({ prompt: '只操作当前画布', sources: [] }) }, './canvas-agent-tools': tools, './canvas-context-trace': trace,
    });
    const state = api.createCanvasAgentState();
    const input = {
        provider, projectId: 'film', sessionId: 'chat', resumeId: options.resumeId,
        config: {}, initialState: state, protocolMessages: [], userText: '读取节点', references: options.references || [], signal: controller.signal,
        getContext: () => ({ project: { id: 'film' }, agentState: state, nodes: [{ id: 'one', title: '节点', type: 'text' }], selectedNodeIds: [] }),
        onModel: (model) => models.push(model), onInvalidate: () => controller.abort(), onSession: (id) => ids.push(id), onText: (text) => texts.push(text),
        onPermission: options.onPermission || (async () => false),
        executeAction: async (action) => { actions.push(action); return options.execute ? options.execute(action, controller) : { ok: true, text: '真实节点' }; },
    };
    return { run: () => runtime.runCanvasLocalAgent(input), input, calls, actions, responses, texts, ids, grants, models, get closed() { return closed; } };
}
test('Grok initializes, authenticates cached login and streams text', async () => {
    const h = harness(); const result = await h.run();
    assert.equal(result.reply, '你好'); assert.deepEqual(h.calls.map(c => c.method), ['initialize','authenticate','session/new','session/prompt']);
    assert.deepEqual(h.ids, ['owned']); assert.equal(h.closed, 1);
});
test('Grok resumes explicit owned session, never newest conversation', async () => {
    const h = harness('grok', { resumeId: 'owned' }); await h.run();
    assert.equal(h.calls[2].method, 'session/load'); assert.equal(h.calls[2].params.sessionId, 'owned');
});
test('Antigravity final response replaces streamed fragments', async () => {
    const h = harness('antigravity'); const result = await h.run();
    assert.equal(result.reply, '最终答案'); assert.equal(h.calls[0].event, 'user'); assert.equal(typeof h.calls[0].message.content, 'string');
});
test('images fail before opening a local provider, without silent downgrade', async () => {
    for (const provider of ['grok','antigravity']) {
        const h = harness(provider, { references: [{ id: 'image', dataUrl: 'local-ref:original' }] });
        await assert.rejects(h.run, /只支持文字/); assert.equal(h.calls.length, 0); assert.equal(h.closed, 0);
    }
});
test('MCP calls use real action normalization and return actual result', async () => {
    const h = harness('antigravity', {
        onTurn: async ({ emit }) => emit({ event: 'canvas_tool', id: '1', name: 'get_node', arguments: { nodeId: 'one' } }),
        onResponse: ({ emit }) => emit({ event: 'result', result: { status: 'SUCCESS', response: '已读取' } }),
    });
    const result = await h.run(); assert.equal(result.reply, '已读取'); assert.equal(h.actions.length, 1); assert.equal(h.responses[0].result.text, '真实节点');
});
test('layout changes require an explicit request', async () => {
    const h = harness('antigravity', {
        onTurn: async ({ emit }) => emit({ event: 'canvas_tool', id: '1', name: 'arrange_nodes', arguments: {} }),
        onResponse: ({ emit }) => emit({ event: 'result', result: { status: 'SUCCESS', response: '保留位置' } }),
    });
    await h.run(); assert.equal(h.actions.length, 0); assert.equal(h.responses[0].result.ok, false);
});
test('disconnect invalidates an in-flight confirmation before a late approval', async () => {
    let release;
    const permission = new Promise(resolve => { release = resolve; });
    const h = harness('grok', {
        onPermission: () => permission,
        onTurn: async ({ emit, listener }) => {
            emit({ id: 'permission', method: 'session/request_permission', params: { options: [{ kind: 'allow_once', optionId: 'once' }], toolCall: { title: '外部操作' } } });
            await new Promise(resolve => setTimeout(resolve, 0));
            listener.onClose(new Error('断开')); release(true);
        },
    });
    await assert.rejects(h.run); assert.equal(h.grants.length, 0); assert.equal(h.closed, 1);
});
test('failed Antigravity result is never reported as success', async () => {
    const h = harness('antigravity', { onTurn: async ({ emit }) => emit({ event: 'result', result: { status: 'ERROR', error: 'quota exhausted' } }) });
    await assert.rejects(h.run, /quota exhausted/); assert.equal(h.closed, 1);
});
test('non-end-turn Grok result is not retried or reported as complete', async () => {
    const h = harness('grok', { stopReason: 'cancelled' }); await assert.rejects(h.run, /未正常完成/); assert.equal(h.calls.filter(c => c.method === 'session/prompt').length, 1);
});
test('the actual panel queues permission and media confirmations, abort drops later prompts', async () => {
    const source = readFileSync(new URL('../../web/src/app/(user)/canvas/components/canvas-assistant-panel.tsx', import.meta.url), 'utf8');
    const ast = ts.createSourceFile('panel.tsx', source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const statements = new Map();
    function visit(node) {
        if (ts.isVariableStatement(node)) for (const declaration of node.declarationList.declarations) {
            const name = declaration.name.getText(ast);
            if (name === 'confirmationQueue' || name === 'requestConfirmation') statements.set(name, node.getText(ast));
        }
        ts.forEachChild(node, visit);
    }
    visit(ast);
    assert.equal(statements.size, 2);
    const compiled = ts.transpileModule(`module.exports = ({controller,pendingDeleteRef,setPendingDelete}) => { ${statements.get('confirmationQueue')} ${statements.get('requestConfirmation')} return requestConfirmation; };`, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
    const module = { exports: {} }; vm.runInNewContext(compiled, { module });
    const shown = [], controller = new AbortController();
    const ask = module.exports({ controller, pendingDeleteRef: { current: null }, setPendingDelete: (value) => shown.push(value) });
    const first = ask({ title: '工具许可', permission: true });
    const second = ask({ title: '媒体生成', media: true });
    await new Promise(resolve => setTimeout(resolve, 0)); assert.equal(shown.length, 1);
    shown[0].resolve(true); assert.equal(await first, true);
    await new Promise(resolve => setTimeout(resolve, 0)); assert.equal(shown.length, 2);
    const third = ask({ title: '之后的操作' });
    controller.abort(); shown[1].resolve(false);
    assert.equal(await second, false); assert.equal(await third, false); assert.equal(shown.length, 2);
});

test('Antigravity startup failure sends no model message', async () => {
    const h = harness('antigravity', {initError:'agent missing; fallback blocked'});
    await assert.rejects(h.run, /停止|fallback/); assert.equal(h.calls.length,0);
});

test('models come from Grok native session metadata, AGY remains unknown', async () => {
    const grok = harness(); await grok.run(); assert.deepEqual(grok.models,['grok-4.6']);
    const agy = harness('antigravity'); await agy.run(); assert.deepEqual(agy.models,[]);
});
