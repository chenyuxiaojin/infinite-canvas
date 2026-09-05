// Read-only real conversation corpus; output contains aggregate measurements,
// never message bodies. Measures actual Markdown parse + server rendering only,
// not WebKit layout, installed App startup or end-to-end interaction latency.
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { writeFileSync } from 'node:fs';
import os from 'node:os';
const requireWeb = createRequire(new URL('../../web/package.json',import.meta.url));
const {default: ReactMarkdown} = await import(pathToFileURL(requireWeb.resolve('react-markdown')));
const {default: remarkGfm} = await import(pathToFileURL(requireWeb.resolve('remark-gfm')));
const React = requireWeb('react');
const {renderToStaticMarkup} = requireWeb('react-dom/server');
const database = process.argv[2];
const output = process.argv[3];
if (!database || !output) throw Error('Pass explicit read-only database and output report paths');
const rows=JSON.parse(execFileSync('/usr/bin/sqlite3',['-json',`file:${database}?mode=ro`,"SELECT project_data FROM canvas_projects WHERE json_valid(project_data)"],{encoding:'utf8',maxBuffer:128*1024*1024}));
const sessions=rows.flatMap(row=>JSON.parse(row.project_data).chatSessions||[]);
const messages=[...sessions].sort((a,b)=>b.messages.reduce((sum,m)=>sum+(m.text?.length||0),0)-a.messages.reduce((sum,m)=>sum+(m.text?.length||0),0))[0]?.messages||[];
if (!messages.length) throw Error('No real conversation');
function run(list) {const start=performance.now();let chars=0;for(const message of list)if(message.role==='assistant'&&message.text)chars+=renderToStaticMarkup(React.createElement(ReactMarkdown,{remarkPlugins:[remarkGfm],skipHtml:true},message.text)).length;return {ms:performance.now()-start,chars};}
function measure(list){const firstPass=run(list);for(let n=0;n<3;n++)run(list);const samples=Array.from({length:12},()=>run(list));const times=samples.map(x=>x.ms).sort((a,b)=>a-b);return {firstPassMs:firstPass.ms,parsedTextCharacters:list.filter(m=>m.role==='assistant').reduce((sum,m)=>sum+(m.text?.length||0),0),renderedMessages:list.length,parsedMarkdownMessages:list.filter(m=>m.role==='assistant'&&m.text).length,medianMs:times[Math.floor(times.length/2)],minMs:times[0],maxMs:times.at(-1),outputChars:samples[0].chars,samples:12};}
if (process.argv[4]) {
 console.log(JSON.stringify(measure(process.argv[4] === 'before' ? messages : messages.slice(-12))));
 process.exit(0);
}
function isolated(mode) {return JSON.parse(execFileSync(process.execPath,[fileURLToPath(import.meta.url),database,output,mode],{encoding:'utf8'}));}
const report={environment:{platform:os.platform(),arch:os.arch(),cpu:os.cpus()[0].model,node:process.version},database,corpus:{sessions:sessions.length,messages:messages.length,textCharacters:messages.reduce((sum,m)=>sum+(m.text?.length||0),0)},before:isolated('before'),after:isolated('after'),limitation:'Real corpus and installed ReactMarkdown, separate fresh Node processes for before/after parse+server-render timing; firstPassMs excludes module loading and database reading. No native App or browser layout/performance claim.'};
writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));
