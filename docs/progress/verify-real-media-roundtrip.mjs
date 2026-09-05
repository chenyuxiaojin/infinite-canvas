// Read-only source files and an explicit SQLite snapshot; writes only a new evidence directory.
// Real frontend ZIP modules with isolated storage adapters. This is not native IPC/UI acceptance.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { createHash, webcrypto, randomUUID } from 'node:crypto';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import vm from 'node:vm';
const [snapshot, registryPath, output] = process.argv.slice(2);
assert.ok([snapshot,registryPath,output].every(p=>p&&path.isAbsolute(p)));
assert.ok(!existsSync(output),'Evidence directory must be new');
mkdirSync(output,{recursive:true,mode:0o700});
const requireWeb=createRequire(new URL('../../web/package.json',import.meta.url));
const ts=requireWeb('typescript');
function load(relative,imports) {
 const source=readFileSync(new URL('../../web/src/'+relative,import.meta.url),'utf8');
 const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const module={exports:{}};
 vm.runInNewContext(js,{module,exports:module.exports,Blob,ArrayBuffer,Uint8Array,crypto:webcrypto,console,require:name=>{assert.ok(name in imports,`Unexpected import ${name}`);return imports[name]}});
 return module.exports;
}
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const row=JSON.parse(execFileSync('/usr/bin/sqlite3',['-json',`file:${snapshot}?mode=ro`,"SELECT project_data FROM canvas_projects WHERE id='DUkqxVcwRh30uwMAskyxt'"],{encoding:'utf8',maxBuffer:16*1024*1024}))[0];
const project=JSON.parse(row.project_data), original=structuredClone(project);
project.title+=' · 原图混合往返隔离验证';
const roots=JSON.parse(readFileSync(registryPath,'utf8')).roots;
const references=new Map();
function walk(value){if(!value||typeof value!=='object')return;if(value.rootId&&value.relativePath&&value.storageKey)references.set(value.storageKey,value);Object.values(value).forEach(walk)}
walk(project);
const blobs=new Map(), sources=[];
for(const [key,reference] of references){
 const root=path.resolve(roots[reference.rootId]), file=path.resolve(root,reference.relativePath);assert.ok(file.startsWith(root+path.sep));
 const bytes=readFileSync(file);assert.equal(bytes.length,reference.bytes);assert.equal(sha(bytes),reference.sha256);
 execFileSync('/opt/homebrew/bin/ffmpeg',['-v','error','-xerror','-i',file,'-f','null','-'],{stdio:['ignore','ignore','pipe']});
 blobs.set(key,new Blob([bytes],{type:reference.mimeType}));sources.push({key,bytes:bytes.length,sha256:sha(bytes),mimeType:reference.mimeType});
}
const zip=load('lib/zip.ts',{fflate:requireWeb('fflate')});
const old=await zip.readZip(new Blob([readFileSync(new URL('../../data/p3-evidence/P3-workflow-bedaac2.zip',import.meta.url))]));
const oldManifest=JSON.parse(await old.get('projects.json').text());
const videoItem=oldManifest.projects.flatMap(p=>p.files).find(f=>f.mimeType==='video/mp4');
const extras=[['video','video/mp4',Buffer.from(await old.get(videoItem.path).arrayBuffer())],['audio','audio/wav',readFileSync(new URL('../../data/p3-evidence/p3-test-audio.wav',import.meta.url))]];
for(const [type,mimeType,bytes] of extras){
 const key=type+':real-roundtrip-'+randomUUID(), file=path.join(output,type+(type==='audio'?'.wav':'.mp4'));
 writeFileSync(file,bytes,{mode:0o600});execFileSync('/opt/homebrew/bin/ffmpeg',['-v','error','-xerror','-i',file,'-f','null','-'],{stdio:['ignore','ignore','pipe']});
 blobs.set(key,new Blob([bytes],{type:mimeType}));sources.push({key,bytes:bytes.length,sha256:sha(bytes),mimeType});
 project.nodes.push({id:'qa-'+type,type,title:'中文验收'+type,position:{x:0,y:0},width:320,height:180,metadata:{storageKey:key,content:'blob:isolated-'+type,mimeType}});
 project.connections.push({id:'qa-edge-'+type,fromNodeId:project.nodes[0].id,toNodeId:'qa-'+type});
}
let archive;
const canvas='app/(user)/canvas/utils/';
const exporter=load(canvas+'canvas-export.ts',{'file-saver':{saveAs:blob=>{archive=blob}},'@/lib/zip':zip,'@/services/canvas-media':{readCanvasMediaBlob:async(_,key)=>{assert.ok(blobs.has(key),'Missing '+key);return blobs.get(key)}},'@/services/desktop-runtime':{isDesktopRuntime:()=>false}});
await exporter.exportCanvasProjects([project]);
writeFileSync(path.join(output,'原图与历史-混合素材.zip'),Buffer.from(await archive.arrayBuffer()),{mode:0o600});
const importedBlobs=new Map();const save=async(key,blob)=>{importedBlobs.set(key,blob);return 'blob:'+key};
const importer=load(canvas+'canvas-import.ts',{nanoid:{nanoid:randomUUID},'@/lib/zip':zip,'@/services/image-storage':{setImageBlob:save},'@/services/file-storage':{setMediaBlob:save},'./canvas-export':exporter,'./canvas-graph':load(canvas+'canvas-graph.ts',{})});
const imported=(await importer.importCanvasArchive(archive))[0];
assert.equal(imported.nodes.length,project.nodes.length);assert.deepEqual(JSON.parse(JSON.stringify(imported.connections)),project.connections);
const entries=await zip.readZip(archive),manifest=JSON.parse(await entries.get('projects.json').text());
assert.equal(importedBlobs.size,blobs.size);const importedHashes=[];
for(const blob of importedBlobs.values())importedHashes.push(sha(Buffer.from(await blob.arrayBuffer())));
assert.deepEqual(importedHashes.sort(),sources.map(s=>s.sha256).sort());
assert.ok(exporter.collectStorageKeys(imported).every(k=>!k.startsWith('local-ref:')));
assert.equal(JSON.stringify(imported).includes('"localMedia":'),false);
for(let i=0;i<original.nodes.length;i++){
 const oldNode=original.nodes[i], newNode=imported.nodes[i];assert.equal(newNode.id,oldNode.id);assert.equal(newNode.title,oldNode.title);
 if(oldNode.type==='text')assert.equal(newNode.metadata.content,oldNode.metadata.content);
}
const report={createdAt:new Date().toISOString(),sourceProjectId:original.id,sourceNodes:original.nodes.length,importedNodes:imported.nodes.length,connections:imported.connections.length,registeredOriginals:references.size,files:blobs.size,totalSourceBytes:sources.reduce((n,s)=>n+s.bytes,0),archiveBytes:archive.size,sources,checks:{originalBytesMatchRegistration:true,allSourcesDecode:true,importHashesMatch:true,nestedHistoryRemapped:true,originalTextAndConnectionsPreserved:true},limitations:['Real frontend export/import, isolated storage and media adapters; not native App IPC or IndexedDB acceptance','Video and audio are existing deterministic acceptance assets; image originals are current Case4 4K files','Project ID isolation is performed by the store import action, tested separately']};
writeFileSync(path.join(output,'report.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600});
console.log(JSON.stringify({...report,sources:undefined}));
