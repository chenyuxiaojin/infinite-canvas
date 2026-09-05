// Execute the real settings components with isolated configuration and React SSR.
// No native WebView, credentials, persisted user settings or generation requests.
import assert from 'node:assert/strict';
import test from 'node:test';
import {readFileSync,existsSync} from 'node:fs';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import vm from 'node:vm';
const root=fileURLToPath(new URL('../../web/src/',import.meta.url));
const requireWeb=createRequire(new URL('../../web/package.json',import.meta.url));
const ts=requireWeb('typescript'),React=requireWeb('react');
const {renderToStaticMarkup}=requireWeb('react-dom/server');
const cache=new Map();
function load(file){
 if(cache.has(file))return cache.get(file).exports;
 const module={exports:{}};cache.set(file,module);
 let source=readFileSync(file,'utf8');
 if(file.endsWith('canvas-video-settings-popover.tsx'))source+='\nexport { VideoSettingsPortal };';
 const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX,esModuleInterop:true}}).outputText;
 vm.runInNewContext(compiled,{module,exports:module.exports,console,window:{innerWidth:1148,innerHeight:768},document:{body:{}},require(name){
  if(name==='@/services/api/request')return {apiGet(){throw Error('Network must not run in settings rendering');}};
  if(name==='react-dom')return {...requireWeb(name),createPortal:children=>children};
  if(name.startsWith('@/')||name.startsWith('.')){
   const base=name.startsWith('@/')?path.join(root,name.slice(2)):path.resolve(path.dirname(file),name);
   const resolved=['.ts','.tsx','/index.ts','/index.tsx'].map(x=>base+x).find(existsSync);assert.ok(resolved,name);return load(resolved);
  }
  return requireWeb(name);
 }},{filename:file});return module.exports;
}
const {VideoSettingsPortal}=load(path.join(root,'app/(user)/canvas/components/canvas-video-settings-popover.tsx'));
const settings=load(path.join(root,'stores/use-config-store.ts'));
const {canvasThemes}=load(path.join(root,'lib/canvas-theme.ts'));
const models=[['apimart','kling-v2-6'],['apimart','kling-v3'],['apimart','kling-v2-6-motion-control'],['kie','kling-3-0-video'],...['text-to-video','image-to-video','reference-to-video','transformation'].map(x=>['kie','kling-3-0-omni-'+x]),['kie','kling-2-6-motion-control'],['kie','kling-3-0-motion-control']];
for(const [provider,model] of models)for(const themeName of ['dark','light'])for(const custom of [false,true]){
 test(`${provider}/${model} ${themeName} ${custom?'custom resources':'default'} renders the active portal`,()=>{
  const channel={id:'qa-'+provider,name:provider+' isolated',protocol:provider,models:[model],apiKey:'',baseUrl:'http://127.0.0.1:1'};
  const config={...settings.useConfigStore.getState().config,channelMode:'local',model,videoModel:model,videoChannelId:channel.id,activeChannelId:channel.id,localChannels:[channel],publicChannels:[],models:[model],vquality:'720',size:'16:9',videoSeconds:'5'};
  const props={buttonRect:{left:600,right:700,top:600,bottom:640,width:100,height:40},panelRef:{current:null},placement:'topRight',theme:canvasThemes[themeName],config,onConfigChange(){},onMetadataChange(){},frameOptions:[],resourceOptions:custom?[{nodeId:'text',kind:'text',label:'原始提示',text:'完整正文'},{nodeId:'image',kind:'image',label:'原图'}]:[],metadata:custom?{multiShot:'true',shotType:'customize',klingMultiPrompt:[{textNodeId:'text',duration:'5'}],klingElementList:[{name:'角色',description:'保持原图',nodeIds:['image']}]}:{},visualOnly:false};
  const html=renderToStaticMarkup(React.createElement(VideoSettingsPortal,props));
  assert.ok(html.includes('视频设置'));
  if(model.includes('motion-control'))assert.ok(html.includes('角色朝向参考'));
  else if(model!=='kling-v2-6'&&!model.endsWith('transformation'))assert.ok(html.includes('多镜头分镜'));
  if(custom&&model==='kling-v3')assert.ok(html.includes('分镜提示词'));
 });
}
