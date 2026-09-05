import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { webcrypto } from 'node:crypto';
import test from 'node:test';
import ts from '../../web/node_modules/typescript/lib/typescript.js';
function load(path, imports = {}, globals = {}) {
 const module = { exports: {} };
 vm.runInNewContext(ts.transpileModule(readFileSync(new URL(path, import.meta.url), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText, { module, exports: module.exports, Blob, TextEncoder, Uint8Array, DOMException, queueMicrotask, crypto: webcrypto, require: name => { assert.ok(name in imports, name); return imports[name]; }, ...globals });
 return module.exports;
}
const { createCanvasDragIndex } = load('../../web/src/app/(user)/canvas/utils/canvas-drag-index.ts');
const node = id => ({ id, position: { x: 0, y: 0 } });
const edge = (id, fromNodeId, toNodeId) => ({ id, fromNodeId, toNodeId });
test('drag indexes all endpoints, deduplicates multiselect/group edges and refreshes undo/redo snapshots', () => {
 const index = createCanvasDragIndex();
 const nodes = ['group', 'child', 'outside', 'other'].map(node);
 const connections = [edge('a', 'group', 'child'), edge('b', 'child', 'outside'), edge('c', 'other', 'outside')];
 const result = index(nodes, connections, ['group', 'child']);
 assert.deepEqual([...result.affected].map(x => x.id), ['a', 'b']);
 assert.equal(result.nodesById.get('outside'), nodes[2]);
 const changed = nodes.map(n => n.id === 'child' ? {...n, position: {x: 10, y: 20}} : n);
 assert.equal(index(changed, connections, ['child']).nodesById.get('child').position.x, 10);
 const added = [...connections, edge('d', 'child', 'other')];
 assert.equal(index(changed, added, ['child']).affected.size, 3);
 assert.equal(index(nodes, connections, ['child']).affected.size, 2);
 assert.equal(index(changed, added, ['child']).affected.size, 3);
 assert.equal(index(changed.filter(n => n.id !== 'child'), [], ['child']).affected.size, 0);
 assert.equal(index(nodes, connections, ['child']).nodesById.get('child'), nodes[1]);
});
test('isolated large graph touches only incident edges (operation count, not App performance)', () => {
 const nodes = Array.from({length: 10000}, (_, i) => node(String(i)));
 const edges = nodes.slice(1).map((n, i) => edge(String(i), String(i), n.id));
 const index = createCanvasDragIndex();
 for (let frame = 0; frame < 100; frame++) {
  const result = index(nodes, edges, ['5000']);
  assert.equal(result.affected.size, 2);
 }
});
const base = '../../web/src/app/(user)/canvas/';
const types = load(base + 'types.ts');
const contextModule = load(base + 'agent/canvas-agent-context.ts', {
 '@/lib/audio-generation': {isGlmTtsModel: () => false}, '@/lib/grok-tts': {isGrok2APITtsConfig: () => false},
 '@/lib/gemini': {isGeminiConfig: () => false, isGeminiTtsModel: () => false},
 '@/lib/video-model-capabilities': {supportsVideoAudioGeneration: () => false}, '../types': types,
});
test('context body budget and one-hop related nodes do not depend on edge ordering', () => {
 const input = {projectId: 'isolated', projectTitle: 'test', nodes: Array.from({length: 130}, (_, i) => ({...node(String(i)), title: String(i), type: 'text', metadata: {content: 'original body'}})), selectedNodeIds: ['0'], config: {}, agentState: {approvedNodeIds: [], referenceNodeIds: []}, connections: [edge('a','0','1'),edge('b','1','2')]};
 const first = contextModule.buildCanvasAgentContext(input);
 const reverse = contextModule.buildCanvasAgentContext({...input, connections: [...input.connections].reverse()});
 assert.equal(first.nodes.length, 120);
 assert.deepEqual([...first.nodes.filter(n => n.text).map(n => n.id)], ['0','1']);
 assert.deepEqual([...reverse.nodes.filter(n => n.text).map(n => n.id)], ['0','1']);
 const broad = contextModule.buildCanvasAgentContext({...input, selectedNodeIds: input.nodes.map(n => n.id)});
 assert.equal(broad.nodes.filter(n => n.text).length, 16);
 assert.equal(input.nodes[2].metadata.content, 'original body');
});
const tracing = load(base + 'agent/canvas-context-trace.ts');
test('trace hashes exactly adopted SOP bytes and separates index/body/image and tool reads', async () => {
 const context = {nodes: [{id:'a',title:'目录'}, {id:'b',title:'正文',text:'actual'}]};
 const sources = [{source:'内置 SOP / core.ts',content:'真实规则'}];
 const trace = await tracing.traceCanvasInput(context,sources,['outside']);
 assert.deepEqual([...trace.nodes.map(n=>n.detail)],['index','body','image']);
 const expected = Buffer.from(await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode('真实规则'))).toString('hex');
 assert.equal(trace.sources[0].sha256, expected);
 assert.equal(JSON.stringify(trace).includes('真实规则'),false);
 assert.equal(tracing.traceCanvasTool('get_node',{ok:false,node:{id:'a'}}),undefined);
 assert.equal(tracing.traceCanvasTool('get_node',{ok:true,node:{id:'a',title:'A'}}).kind,'tool');
});
test('stored URL ownership transfers to explicit leases and final release reloads original bytes', () => {
 const revoked = []; let sequence = 0;
 const {StoredObjectUrlCache} = load('../../web/src/services/stored-object-url-cache.ts',{}, {URL: {createObjectURL:()=>`blob:${++sequence}`,revokeObjectURL:url=>revoked.push(url)}});
 const cache = new StoredObjectUrlCache(), blob = new Blob(['original']);
 const first = cache.acquire('a',blob), second = cache.acquire('a',blob);
 assert.equal(first.url,second.url);first.release();assert.equal(revoked.length,0);
 second.release();assert.deepEqual(revoked,[first.url]);second.release();assert.equal(revoked.length,1);
 const again=cache.acquire('a',blob);assert.notEqual(again.url,first.url);
 cache.set('legacy','blob:active-export',blob.size);
 const preview=cache.acquire('legacy',blob);preview.release();
 assert.equal(revoked.includes('blob:active-export'),true);
 again.release();assert.equal(revoked.length,3);
});

test('project/node/fullscreen share original bytes; late loads after leaving allocate no URL', async () => {
 let reads = 0, releases = 0, allocated = 0, resolveRead;
 let delayed = false;
 const original = new Blob([Uint8Array.from([0,255,10,33])],{type:'image/png'});
 const pool = load('../../web/src/services/canvas-media-lease.ts', {
  './media-read-queue': load('../../web/src/services/media-read-queue.ts'),
  './canvas-media': {readCanvasMediaBlob: async()=>original},
  './image-storage': {getImageBlob: async()=> {reads++;if(delayed)return new Promise(resolve=>{resolveRead=resolve});return original;}, leaseStoredImageBlob: (_key,blob)=> {assert.equal(blob,original);allocated++;return {url:'blob:original',release:()=>releases++};}, resolveImageUrl: async()=>{throw Error('unexpected fallback')}},
  './file-storage': {getMediaBlob: async()=>null, leaseStoredMediaBlob:()=>{throw Error('wrong store')},resolveMediaUrl:async()=>''},
 });
 const project=pool.acquireCanvasStoredMedia('image:a','',true,'project');
 const node=pool.acquireCanvasStoredMedia('image:a','',true,'project');
 const fullscreen=pool.acquireCanvasStoredMedia('image:a','',true,'project');
 assert.equal(await project.url,'blob:original');assert.equal(await node.url,'blob:original');assert.equal(reads,1);
 project.release();node.release();assert.equal(releases,0);
 fullscreen.release();assert.equal(releases,1);
 delayed=true;
 const late=pool.acquireCanvasStoredMedia('image:late','',true,'other');
 await new Promise(resolve=>queueMicrotask(resolve));late.release();resolveRead(original);assert.equal(await late.url,'');assert.equal(allocated,1);
});

test('SOP source list exactly matches the selected embedded instruction texts', () => {
 const imports = {'./canvas-agent-context': contextModule};
 for (const name of ['core','workflow','script','image','image-character-sheet','image-storyboard','video','video-extension','video-editing','video-multi-shot','video-single-shot','audio','organize']) imports['./skills/'+name] = load(base+'agent/skills/'+name+'.ts');
 const skills = load(base+'agent/canvas-agent-skills.ts',imports);
 const context = {nodes:[],selectedNodeIds:[],agentState:{brief:''},project:{}};
 const bundle=skills.buildCanvasAgentSkillBundle('references','制作角色四视图',context);
 assert.ok(bundle.sources.some(source=>source.source.endsWith('/image-character-sheet.ts')));
 assert.ok(bundle.sources.every(source=>bundle.prompt.includes(source.content)));
 assert.equal(bundle.sources.some(source=>source.source.includes('unknown')),false);
 assert.equal(skills.buildCanvasAgentSkillPrompt('references','制作角色四视图',context),bundle.prompt);
 assert.equal(bundle.sources.some(source=>source.source.endsWith('/video.ts')),false);
});

const windows = load(base+'utils/canvas-conversation-window.ts');
test('first-open page is bounded, history anchor survives new arrivals, every old message is reachable', () => {
 const messages=Array.from({length:1000},(_,i)=>({id:String(i),text:`正文 ${i}`}));
 const first=windows.conversationWindow(messages,null);
 assert.equal(first.messages.length,12);assert.equal(first.messages.at(-1).id,'999');
 const visited=new Set();let endId=null;
 for(let page=0;page<150;page++) {
  const range=windows.conversationWindow(messages,endId);
  range.messages.forEach(message=>visited.add(message.id));
  if(!range.start)break;
  endId=messages[range.start+windows.CONVERSATION_PAGE_OVERLAP-1].id;
 }
 assert.equal(visited.size,1000);
 const historical=windows.conversationWindow([...messages,{id:'new',text:'最新'}],'499');
 assert.equal(historical.messages.at(-1).id,'499');assert.equal(historical.latest,false);
 assert.deepEqual([...windows.conversationMatches(messages,'正文 995')],[995]);
 assert.equal(windows.conversationWindow(messages,'removed').latest,true);
});

test('asset index keeps persistent keys and custom covers without creating any display URL', () => {
 const {stableAssetMedia,assetMediaReference}=load('../../web/src/services/asset-media-reference.ts');
 const asset=Object.freeze({kind:'image',coverUrl:'blob:old',data:Object.freeze({storageKey:'image:original',dataUrl:'blob:old'})});
 const stable=stableAssetMedia(asset);
 assert.equal(stable.data.storageKey,'image:original');assert.equal(stable.data.dataUrl,'');assert.equal(stable.coverUrl,'');
 assert.equal(asset.data.dataUrl,'blob:old');
 const custom={...asset,coverUrl:'https://example.invalid/cover.png'};
 assert.equal(stableAssetMedia(custom).coverUrl,custom.coverUrl);
 assert.equal(assetMediaReference(custom,true).storageKey,undefined);
 assert.equal(assetMediaReference(stable).storageKey,'image:original');
});

test('project scope cancels late work and closes independently of other consumers', async () => {
 let calls=0,releases=0,resolve;
 const {CanvasMediaScope}=load('../../web/src/services/canvas-media-scope.ts',{
  './image-storage':{adoptStoredImageUrl:()=>undefined}, './file-storage':{adoptStoredMediaUrl:()=>undefined},
  './canvas-media-lease':{acquireCanvasStoredMedia:()=>{calls++;return {url:new Promise(done=>{resolve=done}),release:()=>releases++};}},
 });
 const scope=new CanvasMediaScope('project');const pending=scope.url('image:one');
 scope.close();resolve('blob:original');await assert.rejects(pending,/画布已关闭/);
 await assert.rejects(scope.url('image:late'),/画布已关闭/);
 assert.equal(calls,1);assert.equal(releases,1);
});

test('media read queue has at most two full reads and skips abandoned queued views', async () => {
 const {createMediaReadQueue}=load('../../web/src/services/media-read-queue.ts');
 const queue=createMediaReadQueue(2);const finishes=[];let active=0,peak=0,calls=0,alive=true;
 const read=()=>{calls++;active++;peak=Math.max(peak,active);return new Promise(resolve=>finishes.push(()=>{active--;resolve('original')}));};
 const first=queue(read,()=>true),second=queue(read,()=>true),third=queue(read,()=>alive);
 await new Promise(resolve=>queueMicrotask(resolve));assert.equal(calls,2);alive=false;
 finishes.forEach(done=>done());await Promise.all([first,second,third]);
 assert.equal(peak,2);assert.equal(calls,2);assert.equal(await third,'');
});

test('display reacquisition never returns a revoked source while the replacement is pending', async () => {
 const cells=[]; let cursor=0, effects=[], pending=[], sequence=0, released=[];
 const react={
  useState(initial){const i=cursor++;if(!(i in cells))cells[i]=initial;return [cells[i],value=>{cells[i]=value}];},
  useRef(initial){const i=cursor++;if(!(i in cells))cells[i]={current:initial};return cells[i];},
  useCallback(callback){cursor++;return callback;},
  useEffect(fn,deps){const i=cursor++;const old=cells[i];if(!old||deps.some((x,j)=>x!==old.deps[j]))effects.push(()=>{old?.cleanup?.();cells[i]={deps,cleanup:fn()};});},
 };
 const {useStoredMediaSource}=load('../../web/src/hooks/use-stored-media-source.ts',{
  react, '@/services/canvas-media-lease':{acquireCanvasStoredMedia:()=>{const id=++sequence;return {url:new Promise(resolve=>pending.push(()=>resolve(`blob:${id}`))),release:()=>released.push(id)}}},
 });
 function render(enabled){cursor=0;const result=useStoredMediaSource({storageKey:'image:test',enabled});const todo=effects;effects=[];todo.forEach(fn=>fn());return result;}
 assert.equal(render(true).src,'');pending.shift()();await Promise.resolve();assert.equal(render(true).src,'blob:1');
 assert.equal(render(false).src,'');assert.deepEqual(released,[1]);
 assert.equal(render(true).src,'');assert.equal(render(true).src,'');
 pending.shift()();await Promise.resolve();assert.equal(render(true).src,'blob:2');
 render(false);assert.deepEqual(released,[1,2]);
});

test('replacement and deletion retain active readers until each original lease closes',()=>{
 const revoked=[];let serial=0;
 const {StoredObjectUrlCache}=load('../../web/src/services/stored-object-url-cache.ts',{}, {URL:{createObjectURL:()=>`blob:${++serial}`,revokeObjectURL:url=>revoked.push(url)}});
 const cache=new StoredObjectUrlCache();const original=cache.acquire('key',new Blob(['original']));
 cache.set('key','blob:replacement');const replacement=cache.adopt('key');cache.delete('key');
 assert.deepEqual(revoked,[]);original.release();assert.deepEqual(revoked,[original.url]);
 replacement.release();replacement.release();assert.deepEqual(revoked,[original.url,'blob:replacement']);
});
