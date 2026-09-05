// Real modules, isolated storage/IPC: no live app, project, model or media writes.
import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {webcrypto} from 'node:crypto';
import vm from 'node:vm';
const requireWeb=createRequire(new URL('../../web/package.json',import.meta.url));
const ts=requireWeb('typescript');
const {create}=requireWeb('zustand');
const {persist}=requireWeb('zustand/middleware');
let requestSequence=0;
const tick=(ms=0)=>new Promise(resolve=>setTimeout(resolve,ms));
function load(path,imports,globals={}) {
 const source=readFileSync(new URL('../../web/src/'+path,import.meta.url),'utf8');
 const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const module={exports:{}};
 const context=vm.createContext({module,exports:module.exports,console,Blob,URL,ArrayBuffer,Uint8Array,crypto:webcrypto,setTimeout,clearTimeout,structuredClone,...globals});
 context.require=name=>{
  if(name==="../protocol/canvas-operation-protocol") return load("app/(user)/canvas/protocol/canvas-operation-protocol.ts",{});
  assert.ok(name in imports,`Unexpected import ${name}`);
  // Zustand checks instanceof Promise: evaluate it in the source module's realm.
  if(name==='zustand/middleware') return vm.runInContext('(function(){const exports={};'+readFileSync(requireWeb.resolve('zustand/middleware'),'utf8')+';return exports})()',context);
  if(name==='fast-deep-equal') return vm.runInContext('(function(){const module={exports:{}};'+readFileSync(requireWeb.resolve('fast-deep-equal'),'utf8')+';return {default:module.exports}})()',context);
  return imports[name];
 };
 vm.runInContext(js,context);
 return module.exports;
}
const canvasPath='app/(user)/canvas/';
const graph=load(canvasPath+'utils/canvas-graph.ts',{});
const original={id:'audit-film',title:'audit',createdAt:'2026-01-01T00:00:00Z',updatedAt:'2026-01-01T00:00:00Z',nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'saved'}}],connections:[],chatSessions:[],viewport:{x:0,y:0,k:1},sidePanel:{open:true,width:320},agentPanel:{open:false,width:390},__desktopRevision:'base'};
async function storeHarness({desktop=true,deleted=[],local=[original],storage,fail=false,database:initialDatabase}={}) {
 const values=storage||new Map([['infinite-canvas:canvas_store',JSON.stringify({state:{projects:local},version:0})]]);
 const database=initialDatabase||new Map([[original.id,structuredClone(original)]]);const writes=[];let failure=fail;let localFailure=false;let beforeSave=async()=>{};let response=value=>value;let beforeRestore=async()=>{};const restores=[];
 const service={restoreDesktopCanvasVersion:async(id,sequence,expectedRevision,requestId)=>{ restores.push({id,sequence,expectedRevision,requestId});await beforeRestore();const current=database.get(id);if(current.__desktopRevision!==expectedRevision)throw Error('REVISION_CONFLICT');const saved={...structuredClone(original),__desktopRevision:'restored-'+sequence};database.set(id,saved);return structuredClone(saved);},isDesktopRuntime:()=>desktop,loadDesktopCanvasDeletedIds:async()=>deleted,loadDesktopCanvasProjects:async()=>[...database.values()],saveDesktopCanvasProject:async project=>{
  writes.push(structuredClone(project));await beforeSave(project);if(failure)throw Error('injected disk failure');
  const current=database.get(project.id);if(current&&current.__desktopRevision!==project.__desktopRevision)throw Error('REVISION_CONFLICT');
  const saved={...structuredClone(project),__desktopRevision:'revision-'+writes.length};database.set(project.id,saved);return response(structuredClone(saved));
 }};
 const {useCanvasStore:store}=load(canvasPath+'stores/use-canvas-store.ts',{
  zustand:{create},'zustand/middleware':{persist},nanoid:{nanoid:()=> 'recovered-film-'+(++requestSequence)},'fast-deep-equal':{default:requireWeb('fast-deep-equal')},'../utils/canvas-graph':graph,
  '@/lib/localforage-storage':{localForageStorage:{getItem:async key=>values.get(key)||null,setItem:async(key,value)=>{if(localFailure)throw Error('injected local storage failure');values.set(key,value)},removeItem:async key=>{values.delete(key)}}},
  '@/services/api/canvas-tasks':{},'@/services/api/user-config':{},'@/stores/use-user-store':{useUserStore:{getState:()=>({token:''})}},'@/services/desktop-runtime':service,
 });
 for(let n=0;n<100&&!store.getState().hydrated;n++)await tick(1);
 assert.equal(store.getState().hydrated,true);
 return {store,values,writes,database,restores,beforeRestore:hook=>{beforeRestore=hook},setFailure:value=>{failure=value},setLocalFailure:value=>{localFailure=value},beforeSave:hook=>{beforeSave=hook},response:hook=>{response=hook}};
}

test('failed desktop save survives refresh and restart, then an explicit retry saves it',async()=>{
 const h=await storeHarness({fail:true});
 h.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'keep my edit'}}]});
 await tick(460);assert.equal(h.store.getState().saveStatus[original.id].state,'error');
 await assert.rejects(h.store.getState().refreshFromDesktop(),/disk failure/);
 assert.equal(h.store.getState().projects[0].nodes[0].metadata.content,'keep my edit');
 const restarted=await storeHarness({storage:h.values,fail:true});
 assert.equal(restarted.store.getState().projects[0].nodes[0].metadata.content,'keep my edit');
 restarted.setFailure(false);await restarted.store.getState().retrySave(original.id);
 assert.equal(restarted.database.get(original.id).nodes[0].metadata.content,'keep my edit');
 assert.deepEqual(JSON.parse(restarted.values.get('infinite-canvas:recovery:index')),[]);
});

test('refresh must not rebase pending edits onto another writer without a conflict',async()=>{
 const h=await storeHarness();
 h.store.getState().updateProject(original.id,{title:'my title',nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'my edit'}}]});
 h.database.set(original.id,{...original,__desktopRevision:'someone-else',nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'other edit'}}]});
 await assert.rejects(h.store.getState().refreshFromDesktop(),/REVISION_CONFLICT/);
 assert.equal(h.database.get(original.id).nodes[0].metadata.content,'other edit');
 assert.equal(h.store.getState().projects[0].nodes[0].metadata.content,'my edit');
});

test('deleted desktop projects disappear from active list while original local records are retained',async()=>{
 const gone={...original,id:'deleted-film'};
 const h=await storeHarness({local:[original,gone],deleted:[gone.id]});
 assert.deepEqual(Array.from(h.store.getState().projects,p=>p.id),[original.id]);
 assert.equal(h.writes.length,0);
 assert.equal(JSON.parse(h.values.get('infinite-canvas:recovery:deleted:'+gone.id)).id,gone.id);
});

test('UI-only viewport and panel choices survive without changing content timestamps',async()=>{
 const h=await storeHarness({desktop:false});
 h.store.getState().updateProject(original.id,{viewport:{x:321,y:456,k:.05},sidePanel:{open:false,width:400},agentPanel:{open:true,width:500}});
 await tick(460);
 const restarted=await storeHarness({desktop:false,storage:h.values});const p=restarted.store.getState().projects[0];
 assert.equal(p.viewport.k,.05);assert.equal(p.sidePanel.open,false);assert.equal(p.agentPanel.open,true);assert.equal(p.updatedAt,original.updatedAt);assert.equal(h.writes.length,0);
});

test('an explicit empty project index does not resurrect the legacy whole-store copy',async()=>{
 const values=new Map([['infinite-canvas:canvas_store',JSON.stringify({state:{projects:[original]}})],['infinite-canvas:canvas_store:index',JSON.stringify({version:1,ids:[]})]]);
 const h=await storeHarness({desktop:false,storage:values});assert.equal(h.store.getState().projects.length,0);
});

test('rapid edits during a delayed save use the new revision and persist the final edit',async()=>{
 const h=await storeHarness();let release;const gate=new Promise(resolve=>{release=resolve});
 h.beforeSave(async()=>{if(h.writes.length===1)await gate});
 h.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'first'}}]});
 const saving=h.store.getState().retrySave(original.id);
 for(let i=0;i<100&&!h.writes.length;i++)await tick(1);
 for(let i=0;i<25;i++)h.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'latest '+i}}]});
 assert.equal(h.store.getState().saveStatus[original.id].state,'pending');release();await saving;
 assert.equal(h.database.get(original.id).nodes[0].metadata.content,'latest 24');
 assert.equal(h.store.getState().projects[0].nodes[0].metadata.content,'latest 24');
 assert.equal(h.writes.length,2);assert.equal(h.writes[1].__desktopRevision,'revision-1');assert.equal(h.store.getState().saveStatus[original.id].state,'saved');
});

test('JSON object key order in a real IPC response does not cause a false save conflict',async()=>{
 const h=await storeHarness();
 h.response(project=>{const node=project.nodes[0];project.nodes[0]=Object.fromEntries(Object.entries(node).reverse());return project;});
 h.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'changed'}}],viewport:{k:2,y:20,x:10}});await h.store.getState().retrySave(original.id);
 assert.equal(h.store.getState().saveStatus[original.id].state,'saved');
});

test('browser persistence failures are visible and retry writes the current snapshot',async()=>{
 const h=await storeHarness({desktop:false});h.setLocalFailure(true);
 h.store.getState().updateProject(original.id,{viewport:{x:88,y:99,k:.2}});await tick(460);
 assert.equal(h.store.getState().saveStatus[original.id].state,'error');h.setLocalFailure(false);await h.store.getState().retrySave(original.id);
 assert.equal(JSON.parse(h.values.get('infinite-canvas:canvas_project:'+original.id)).viewport.x,88);
 assert.equal(h.store.getState().saveStatus[original.id].state,'saved');
});

test('invalid graph writes are rejected and retained for repair without touching saved content',async()=>{
 const h=await storeHarness();
 h.store.getState().updateProject(original.id,{connections:[{id:'bad',fromNodeId:'n',toNodeId:'missing'}]});
 await assert.rejects(h.store.getState().retrySave(original.id),/节点不存在/);
 assert.equal(h.database.get(original.id).connections.length,0);assert.equal(h.store.getState().projects[0].connections.length,1);
 assert.throws(()=>h.store.getState().importProject({nodes:[{id:'same'},{id:'same'}],connections:[]}),/重复节点/);
});


test('a committed write with a lost reply is recovered only when every saved field matches',async()=>{
 const h=await storeHarness();h.response(()=>{throw Error('reply lost after commit')});
 h.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'already committed'}}]});
 await assert.rejects(h.store.getState().retrySave(original.id),/reply lost/);
 const restarted=await storeHarness({storage:h.values,database:h.database});
 assert.equal(restarted.store.getState().projects[0].nodes[0].metadata.content,'already committed');
 assert.equal(restarted.store.getState().projects[0].__desktopRevision,'revision-1');
 await restarted.store.getState().retrySave(original.id);assert.equal(restarted.writes.length,0);
 assert.deepEqual(JSON.parse(restarted.values.get('infinite-canvas:recovery:index')),[]);
});

test('history restore flushes the current edit, receives the latest revision, and persists the restored snapshot',async()=>{
 const h=await storeHarness();h.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'new edit'}}]});
 await h.store.getState().restoreVersion(original.id,7);
 assert.equal(h.writes.length,1);assert.equal(h.restores[0].expectedRevision,'revision-1');assert.match(h.restores[0].requestId,/^[a-f0-9-]{36}$/);
 assert.equal(h.database.get(original.id).__desktopRevision,'restored-7');assert.equal(h.store.getState().restoredRevisions[original.id],'restored-7');assert.equal(h.store.getState().projects[0].nodes[0].metadata.content,'saved');
 assert.equal(JSON.parse(h.values.get('infinite-canvas:canvas_project:'+original.id)).__desktopRevision,'restored-7');
 assert.equal(h.store.getState().saveStatus[original.id].state,'saved');
});

test('history preview becomes stale after an edit and active media or chat tasks block restoration',async()=>{
 const stale=await storeHarness();stale.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'edited after preview'}}]});
 await assert.rejects(stale.store.getState().restoreVersion(original.id,1,'base'),/重新预览/);assert.equal(stale.restores.length,0);
 for(const patch of [
  {nodes:[{id:'n',type:'image',metadata:{status:'loading'}}]},
  {chatSessions:[{id:'session',messages:[{status:'running'}]}]},
  {pendingAgentRequest:{prompt:'pending'}},
 ]) {
  const h=await storeHarness();h.store.getState().updateProject(original.id,patch);
  await assert.rejects(h.store.getState().restoreVersion(original.id,1),/停止当前画布/);assert.equal(h.restores.length,0);
 }
});

test('edits arriving during restore survive in recovery and cannot silently overwrite the restored database',async()=>{
 const h=await storeHarness();let release;const gate=new Promise(resolve=>{release=resolve});h.beforeRestore(()=>gate);
 const restoring=h.store.getState().restoreVersion(original.id,3);
 for(let i=0;i<100&&!h.restores.length;i++)await tick(1);
 h.store.getState().updateProject(original.id,{nodes:[{id:'n',type:'text',title:'原节点',position:{x:0,y:0},width:240,height:160,metadata:{content:'edit while restoring'}}]});
 await tick(450);assert.equal(h.writes.length,0);release();await assert.rejects(restoring,/恢复期间又有新编辑/);
 assert.equal(h.database.get(original.id).nodes[0].metadata.content,'saved');
 assert.equal(h.store.getState().projects[0].nodes[0].metadata.content,'edit while restoring');
 assert.equal(JSON.parse(h.values.get('infinite-canvas:recovery:project:'+original.id)).nodes[0].metadata.content,'edit while restoring');
 assert.equal(h.store.getState().saveStatus[original.id].state,'error');
 await assert.rejects(h.store.getState().retrySave(original.id),/REVISION_CONFLICT/);
});

function mediaHarness({failure=false}={}) {
 const bytes=Uint8Array.from([137,80,78,71,13,10,26,10,1,2,3]).buffer;const calls=[];
 class Reader {readAsDataURL(blob){blob.arrayBuffer().then(buffer=>{this.result=`data:${blob.type};base64,${Buffer.from(buffer).toString('base64')}`;this.onload()}).catch(()=>this.onerror())}}
 const media=load('services/canvas-media.ts',{'@tauri-apps/api/core':{isTauri:()=>true,invoke:async(command,args)=>{calls.push({command,args});if(failure)throw Error('missing original');return bytes}},'@/services/file-storage':{getMediaBlob:async()=>null},'@/services/image-storage':{getImageBlob:async()=>null,imageToDataUrl:async ref=>ref.dataUrl}},{FileReader:Reader});
 return {media,bytes,calls};
}

test('local-ref model input resolves exact image bytes and a failed read aborts sending',async()=>{
 const h=mediaHarness();const ref={id:'img',title:'原图',dataUrl:'local-ref:asset-a',storageKey:'local-ref:asset-a',mimeType:'image/png'};
 const refs=await h.media.resolveCanvasModelReferences('film',[ref]);
 assert.deepEqual(Buffer.from(refs[0].dataUrl.split(',')[1],'base64'),Buffer.from(h.bytes));
 assert.equal(ref.dataUrl,'local-ref:asset-a');assert.equal(h.calls[0].args.projectId,'film');
 await assert.rejects(mediaHarness({failure:true}).media.resolveCanvasModelReferences('film',[ref]),/原图.*missing original/);
});

test('local-ref export embeds and hashes media; import remaps it and rejects missing bytes',async()=>{
 const h=mediaHarness();let archive;
 const zip=load('lib/zip.ts',{fflate:requireWeb('fflate')});
 const exporter=load(canvasPath+'utils/canvas-export.ts',{'file-saver':{saveAs:blob=>{archive=blob}},'@/lib/zip':zip,'@/services/canvas-media':h.media,'@/services/desktop-runtime':{isDesktopRuntime:()=>false}});
 const project={...original,nodes:[{id:'img',type:'image',metadata:{content:'local-ref:asset-a',storageKey:'local-ref:asset-a',mimeType:'image/png',localMedia:{storageKey:'local-ref:asset-a',rootId:'agent-media'}}}]};
 await exporter.exportCanvasProjects([project]);const entries=await zip.readZip(archive);const manifest=JSON.parse(await entries.get('projects.json').text());
 assert.equal(manifest.projects[0].files.length,1);assert.equal(manifest.projects[0].files[0].bytes,h.bytes.byteLength);assert.match(manifest.projects[0].files[0].sha256,/^[a-f0-9]{64}$/);
 const importedBytes=[];
 const importer=load(canvasPath+'utils/canvas-import.ts',{nanoid:{nanoid:()=> 'fresh-id'},'@/lib/zip':zip,'@/services/image-storage':{setImageBlob:async(key,blob)=>{importedBytes.push({key,blob});return 'blob:fresh'}},'@/services/file-storage':{setMediaBlob:async()=>{throw Error('wrong media store')}},'./canvas-export':exporter,'./canvas-graph':graph});
 const imported=await importer.importCanvasArchive(archive);assert.equal(imported[0].nodes[0].metadata.storageKey,'image:fresh-id');assert.equal(imported[0].nodes[0].metadata.content,'blob:fresh');assert.equal(imported[0].nodes[0].metadata.localMedia,undefined);
 assert.deepEqual(Buffer.from(await importedBytes[0].blob.arrayBuffer()),Buffer.from(h.bytes));
 const incomplete=await zip.createZip([{name:'projects.json',data:JSON.stringify(manifest)}]);await assert.rejects(importer.importCanvasArchive(incomplete),/缺失/);
});
