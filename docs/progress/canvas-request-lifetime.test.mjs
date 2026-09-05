import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import vm from 'node:vm';
import test from 'node:test';
import assert from 'node:assert/strict';
const require=createRequire(new URL('../../web/package.json',import.meta.url));
const ts=require('typescript');
const module={exports:{}};
const js=ts.transpileModule(readFileSync(new URL('../../web/src/services/api/request-lifetime.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
vm.runInNewContext(js,{module,exports:module.exports,AbortController,ReadableStream,Error,setTimeout,clearTimeout});
const {requestLifetime,forwardResponseBody}=module.exports;
const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));
test('cancellation propagates to a stalled reader and releases source',async()=>{
 const parent=new AbortController();const life=requestLifetime(parent.signal,1000);let cancelled=false;
 const stream=forwardResponseBody(new ReadableStream({cancel(){cancelled=true;}}),life);
 const pending=stream.getReader().read();parent.abort(new Error('user cancelled'));
 await assert.rejects(pending,/user cancelled/);assert.equal(cancelled,true);
});
test('idle timeout is distinct and cancels backend',async()=>{
 const parent=new AbortController();const life=requestLifetime(parent.signal,20);let cancelled=false;
 const stream=forwardResponseBody(new ReadableStream({cancel(){cancelled=true;}}),life);
 await assert.rejects(stream.getReader().read(),e=>e.kind==='read_timeout');assert.equal(cancelled,true);
});
test('continuous stream exceeds idle duration and ends with exact bytes',async()=>{
 const parent=new AbortController();const life=requestLifetime(parent.signal,60);let n=0;
 const source=new ReadableStream({async pull(c){await sleep(15);if(n===8)c.close();else c.enqueue(new Uint8Array([n++]));}});
 const reader=forwardResponseBody(source,life).getReader();const bytes=[];
 while(true){const r=await reader.read();if(r.done)break;bytes.push(...r.value);}
 assert.deepEqual(bytes,[0,1,2,3,4,5,6,7]);await sleep(75);assert.equal(life.signal.aborted,false);
});
test('downstream cancellation stops upstream without retry',async()=>{
 let count=0;const life=requestLifetime(new AbortController().signal,1000);
 const stream=forwardResponseBody(new ReadableStream({cancel(){count++;}}),life);
 await stream.cancel('closed');assert.equal(count,1);assert.equal(life.signal.aborted,true);
});
test('service stream failure is preserved and timer disposed',async()=>{
 const life=requestLifetime(new AbortController().signal,20);
 const stream=forwardResponseBody(new ReadableStream({start(c){c.error(new Error('service exited'));}}),life);
 await assert.rejects(stream.getReader().read(),e=>e.kind==='service_exited');await sleep(30);assert.equal(life.signal.aborted,false);
});

function loadProxy(fetchImpl){
 const module={exports:{}};
 const code=ts.transpileModule(readFileSync(new URL('../../web/src/app/api/[...path]/route.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 vm.runInNewContext(code,{module,exports:module.exports,require:()=>({requestLifetime,forwardResponseBody,RequestFailure:moduleLifetime.RequestFailure}),fetch:fetchImpl,process:{env:{API_BASE_URL:'http://127.0.0.1:9999'}},Headers,Response,crypto:globalThis.crypto,console});
 return module.exports;
}
const moduleLifetime=module.exports;
function incoming(method,signal){return {method,signal,body:method==='POST'?new ReadableStream({start(c){c.close();}}):null,headers:new Headers({'x-request-id':'11111111-1111-4111-8111-111111111111'}),nextUrl:{host:'127.0.0.1:3100',protocol:'http:',search:''}};}
test('proxy carries request ID and cancellation into fetch',async()=>{
 const abort=new AbortController();let forwarded;
 const route=loadProxy(async(_url,init)=>{forwarded=init;return new Response('exact-media');});
 const response=await route.GET(incoming('GET',abort.signal),{params:Promise.resolve({path:['proxy-image']})});
 assert.equal(await response.text(),'exact-media');assert.equal(forwarded.headers.get('x-request-id'),'11111111-1111-4111-8111-111111111111');assert.equal(response.headers.get('x-request-id'),forwarded.headers.get('x-request-id'));
});
test('failed write is not retried and keeps uncertainty plus request ID',async()=>{
 let calls=0;const route=loadProxy(async()=>{calls++;throw new Error('disconnected');});
 const response=await route.POST(incoming('POST',new AbortController().signal),{params:Promise.resolve({path:['prompt-favorites']})});const result=await response.json();assert.equal(calls,1);assert.equal(result.submitted,true);assert.equal(result.kind,'connect_failed');assert.equal(result.requestId,'11111111-1111-4111-8111-111111111111');
});
test('proxy returns cancellation distinctly for an already aborted request',async()=>{
 const abort=new AbortController();abort.abort();let calls=0;
 const route=loadProxy(async(_url,init)=>{calls++;assert.equal(init.signal.aborted,true);throw init.signal.reason;});
 const response=await route.GET(incoming('GET',abort.signal),{params:Promise.resolve({path:['prompts']})});
 assert.equal(response.status,499);assert.equal((await response.json()).kind,'cancelled');assert.equal(calls,1);
});
