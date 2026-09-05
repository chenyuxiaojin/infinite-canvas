// Isolated diagnostics: extracts current source with TypeScript, runs only in-memory mocks.
// No app, UI, network, SQLite, IndexedDB, or model requests are made.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import path from "node:path";
import vm from "node:vm";
import ts from "../../web/node_modules/typescript/lib/typescript.js";

const root = fileURLToPath(new URL("../../", import.meta.url));
const paths = {
  store: "web/src/app/(user)/canvas/stores/use-canvas-store.ts",
  page: "web/src/app/(user)/canvas/[id]/canvas-client-page.tsx",
  canvas: "web/src/app/(user)/canvas/components/infinite-canvas.tsx",
  connections: "web/src/app/(user)/canvas/components/canvas-connections.tsx",
};
const files = Object.fromEntries(Object.entries(paths).map(([key, relative]) => {
  const source = readFileSync(path.join(root, relative), "utf8");
  return [key, { source, ast: ts.createSourceFile(relative, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX) }];
}));
function find(file, name, predicate) {
  let match;
  const walk = (node) => {
    if (predicate(node) && node.name?.getText(file.ast) === name) match = node;
    ts.forEachChild(node, walk);
  };
  walk(file.ast);
  assert.ok(match, `Source definition missing: ${name}`);
  return match;
}
function declaration(key, name) {
  return find(files[key], name, ts.isFunctionDeclaration).getText(files[key].ast).replace(/^export /, "");
}
function callback(key, name) {
  const node = find(files[key], name, ts.isVariableDeclaration);
  assert.ok(ts.isCallExpression(node.initializer));
  return node.initializer.arguments[0].getText(files[key].ast);
}
function objectMethod(key, name) {
  return find(files[key], name, ts.isPropertyAssignment).initializer.getText(files[key].ast);
}
function run(source, context) {
  return vm.runInNewContext(ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
}
function fn(source, context) { return run(`(${source})`, context); }
function clock() {
  let now = 0, nextId = 0;
  const pending = new Map();
  return {
    setTimeout(callback, delay) { const id = ++nextId; pending.set(id, { due: now + delay, callback }); return id; },
    clearTimeout(id) { pending.delete(id); },
    advance(ms) {
      now += ms;
      for (const [id, task] of [...pending]) if (task.due <= now) { pending.delete(id); task.callback(); }
    },
  };
}

const results = [];
const first = { id: "isolated", title: "test", updatedAt: "2026-01-01T00:00:00.000Z", nodes: [], connections: [], viewport: { x: 0, y: 0, k: 1 }, sidePanel: { width: 320 }, agentPanel: { width: 500 } };
for (const patch of [{ viewport: { x: 900, y: 700, k: 0.5 } }, { sidePanel: { width: 480 } }, { agentPanel: { width: 800 } }]) {
  let projects = [first];
  let desktopSaves = 0;
  const writes = [];
  const env = {
    get: () => ({ projects }),
    set: (updater) => { projects = updater({ projects }).projects; },
    queueProjectSave: () => { desktopSaves++; },
    UI_ONLY_PROJECT_KEYS: new Set(["viewport", "sidePanel", "agentPanel"]),
    lastWrittenProjects: new Map([[first.id, first]]),
    canvasShardsReady: true,
    CANVAS_PROJECT_PREFIX: "project:",
    CANVAS_STORE_INDEX_KEY: "index",
    localForageStorage: { async setItem(key, value) { writes.push({ key, value }); }, async removeItem() {} },
  };
  env.isUiOnlyProjectPatch = fn(declaration("store", "isUiOnlyProjectPatch"), env);
  env.projectNeedsWrite = fn(declaration("store", "projectNeedsWrite"), env);
  env.rememberWrittenProjects = fn(declaration("store", "rememberWrittenProjects"), env);
  fn(objectMethod("store", "updateProject"), env)(first.id, patch);
  await fn(declaration("store", "persistLocalProjects"), env)(projects);
  assert.equal(projects[0].updatedAt, first.updatedAt);
  assert.equal(desktopSaves, 0);
  assert.equal(writes.filter((entry) => entry.key === "project:isolated").length, 0);
  results.push({ scenario: `UI-only ${Object.keys(patch)[0]}`, acceptance: "FAIL", observed: { stateUpdated: true, updatedAtPreserved: true, desktopSaveCalls: desktopSaves, projectShardWrites: 0, indexWrites: writes.length }, requirement: "Preserve view/panel choice after reopening without a later content edit" });
}

const timer = clock();
const committed = [];
const viewportRef = { current: { x: 0, y: 0, k: 1 } };
const viewportFn = fn(callback("page", "handleViewportChange"), { viewportRef, viewportIdleTimerRef: { current: null }, setViewport: (value) => committed.push(value), ...timer });
for (let i = 1; i <= 60; i++) { viewportFn({ x: -800 * i / 60, y: 0, k: 1 }); timer.advance(16); }
const beforeIdle = committed.length;
const farNode = { id: "offscreen", position: { x: 1750, y: 50 }, width: 100, height: 100 };
const visible = (viewport) => fn(callback("page", "visibleNodes"), { viewport, containerRef: { current: null }, size: { width: 1200, height: 720 }, nodes: [farNode], nodeById: new Map(), collapsingBatchIds: new Set(), isHiddenBatchChild: () => false })();
assert.equal(beforeIdle, 0);
assert.equal(visible({ x: 0, y: 0, k: 1 }).length, 0);
assert.equal(visible(viewportRef.current).length, 1);
timer.advance(80);
assert.equal(committed.length, 1);
results.push({ scenario: "Continuous pan beyond 280px overscan", acceptance: "FAIL_SOURCE_MODEL_UI_CONFIRMATION_PENDING", observed: { inputEvents: 60, simulatedContinuousMs: 960, viewportStateCommitsDuringGesture: beforeIdle, nodeOnscreenUnderLiveTransform: true, nodeMountedByStaleViewport: false, commitsAfterIdle: committed.length }, limitation: "Fake timers and visibility function; not macOS frame-time measurement" });

let domApplications = 0, scheduledId = 0;
const frames = new Map();
const publish = fn(callback("canvas", "publishViewportRef"), { liveViewportRef: { current: first.viewport }, containerRef: { current: {} }, applyCanvasViewport: () => { domApplications++; }, frameRef: { current: null }, nextViewportRef: { current: null }, onViewportChangeRef: { current: () => {} }, requestAnimationFrame: (fn) => { const id = ++scheduledId; frames.set(id, fn); return id; }, cancelAnimationFrame: (id) => frames.delete(id) });
for (let i = 0; i < 100; i++) publish({ x: i, y: 0, k: 1 });
assert.equal(domApplications, 100);
assert.equal(frames.size, 1);
results.push({ scenario: "Viewport input frame coalescing", acceptance: "PARTIAL", observed: { inputEvents: 100, immediateDomApplyCalls: domApplications, pendingAnimationFrameCallbacks: frames.size }, implication: "Callback coalescing exists; DOM world/grid style updates remain per input event" });

for (const count of [48, 500, 2000]) {
  let pathWrites = 0, pathBuilds = 0, nodeIdReads = 0, scheduled;
  const nodes = Array.from({ length: count }, (_, i) => ({ get id() { nodeIdReads++; return `n${i}`; }, position: { x: i * 30, y: i % 10 * 30 }, width: 100, height: 100 }));
  const groups = Array.from({ length: count - 1 }, (_, i) => ({ dataset: { from: `n${i}`, to: `n${i + 1}` }, querySelectorAll: () => [{ setAttribute: () => { pathWrites++; } }, { setAttribute: () => { pathWrites++; } }] }));
  const build = fn(declaration("connections", "buildConnectionPathD"), {});
  const env = { viewportRef: { current: { x: 0, y: 0, k: 1 } }, dragRef: { current: { isDraggingNode: true, startX: 0, startY: 0, initialSelectedNodes: [{ id: "n0", x: 0, y: 0 }] } }, nodesRef: { current: nodes }, dropTargetGroupIdRef: { current: null }, findGroupDropTarget: () => null, rafRef: { current: null }, requestAnimationFrame: (callback) => { scheduled = callback; return 1; }, cancelAnimationFrame: () => {}, CSS: { escape: (value) => value }, containerRef: { current: { querySelector: () => ({ style: {} }), querySelectorAll: () => groups } }, buildConnectionPathD: (...args) => { pathBuilds++; return build(...args); } };
  const drag = fn(callback("page", "handleGlobalMouseMove"), env);
  drag({ clientX: 200, clientY: 100 }); scheduled();
  const instrumentedIdReads = nodeIdReads;
  env.nodesRef.current = Array.from({ length: count }, (_, i) => ({ id: `n${i}`, position: { x: i * 30, y: i % 10 * 30 }, width: 100, height: 100 }));
  pathBuilds = 0;
  pathWrites = 0;
  const durations = [];
  for (let iteration = 0; iteration < 100; iteration++) {
    const start = performance.now();
    drag({ clientX: 200, clientY: 100 }); scheduled();
    durations.push(performance.now() - start);
  }
  durations.sort((a, b) => a - b);
  assert.equal(pathBuilds, (count - 1) * 100);
  results.push({ scenario: "Drag one node; all displayed edges traversed", acceptance: "RISK_MEASURED_IN_NODE_ONLY", nodes: count, displayedEdges: count - 1, iterations: 100, observed: { pathBuildsPerIteration: pathBuilds / 100, pathWritesPerIteration: pathWrites / 100, nodeIdReadsFromSeparateInstrumentedIteration: instrumentedIdReads, p50Ms: Number(durations[49].toFixed(3)), p95Ms: Number(durations[94].toFixed(3)), maxMs: Number(durations[99].toFixed(3)) }, limitation: "Fake DOM; timing uses ordinary ID properties, excludes WebKit layout/paint, and group-search helper is stubbed. Not app FPS." });
}

const localRefNode = { id: "test-image", type: "image", metadata: { content: "local-ref:asset-test", storageKey: "local-ref:asset-test" } };
const prepared = fn(declaration("page", "prepareCanvasNodes"), {})([localRefNode])[0];
assert.equal(prepared.metadata.content, "local-ref:asset-test");
results.push({ scenario: "local-ref media restoration", acceptance: "FAIL", observed: { prepareLeavesInternalReferenceAsContent: true, hydrationSkippedBecauseContentIsTruthy: true }, evidence: "canvas-client-page.tsx:927,5063–5069; canvas-node.tsx:815; live project has 32 such images" });

console.log(JSON.stringify({ type: "isolated-source-diagnostics", node: process.version, platform: `${process.platform}/${process.arch}`, sources: Object.fromEntries(Object.entries(files).map(([key, file]) => [paths[key], createHash("sha256").update(file.source).digest("hex")])), results }, null, 2));
