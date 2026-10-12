// Real installed Codex CLI + production launch arguments + real mx report/wake owners.
// Synthetic Responses model, isolated git-free installed package and provider home.
// Usage: node codex-installed-idle-probe.mjs ABS_MX ABS_REPOSITORY ABS_ARGUMENT_EXPORTER
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import readline from 'node:readline';
const [mx, repo, exporter] = process.argv.slice(2);
assert([mx,repo,exporter].every(path => path?.startsWith('/')));
const lab = fs.mkdtempSync('/tmp/mx-codex-installed-idle.');
console.log(`fixture ${lab}`);
const pkg = `${lab}/runtime`, home = `${lab}/operational`, provider = `${lab}/provider`, state = `${home}/state`, config = `${lab}/config`;
for (const path of [pkg,home,provider,state,config,`${pkg}/target/release`,`${home}/data`,`${home}/config`,`${home}/projects`]) fs.mkdirSync(path,{recursive:true});
fs.cpSync(`${repo}/bin`,`${pkg}/bin`,{recursive:true});
fs.cpSync(`${repo}/.agents`,`${pkg}/.agents`,{recursive:true});
fs.copyFileSync(`${repo}/AGENTS.md`,`${pkg}/AGENTS.md`);
fs.copyFileSync(mx,`${pkg}/target/release/mx`);
const binary = `${pkg}/target/release/mx`;
fs.writeFileSync(`${pkg}/.multplx-release`,'0.1.0\n');
fs.writeFileSync(`${pkg}/.multplx-config`,`${config}\n`);
fs.writeFileSync(`${config}/root`,`${pkg}\n`);
fs.writeFileSync(`${config}/home`,`${home}\n`);
assert(!fs.existsSync(`${pkg}/.git`) && !fs.existsSync(`${pkg}/.mx-daemon-home`));
const generated = spawnSync(exporter,[pkg,binary,home],{encoding:'utf8'});
assert.equal(generated.status,0,generated.stderr);
const bundle = JSON.parse(generated.stdout);
fs.writeFileSync(`${state}/fixture-child.meta`,bundle.metadata);
const result = `${home}/data/result.md`;
fs.writeFileSync(result,'# Completed isolated child result\n');
const env = {...process.env,CODEX_HOME:provider,MX_ROOT_OVERRIDE:pkg,MX_HOME:home,MX_STATE_OVERRIDE:state,MX_RUST_SOURCE_ROOT:pkg,MX_RUST_BIN:binary,MX_CODEX_IDLE_CLI:'1',MX_CODEX_SHARED_WORKER_HOOKS:'1',MX_NUDGE:'0',MX_POLL:'0.1',MX_SIGNAL_GRACE:'0',MX_HEARTBEAT:'999999',MX_CHECK_INTERVAL:'999999'};
for(const key of ['MX_TASK_ID','MX_CURRENT_ADMISSION_ID','MX_SHIM_DIR','MX_REAL_CODEX','DEEP_REVIEW_GATE','MX_REPORT_STATE_OVERRIDE'])delete env[key];
let phase='bootstrap';
const requests=[];
const toolCalls=[];
const completions=[];
function stream(response,items){
 response.writeHead(200,{'Content-Type':'text/event-stream'});
 response.end([{type:'response.created',response:{id:`r${requests.length}`}},...items.map(item=>({type:'response.output_item.done',item})),{type:'response.completed',response:{id:`r${requests.length}`,usage:{input_tokens:0,output_tokens:0,total_tokens:0}}}].map(item=>`data: ${JSON.stringify(item)}\n\n`).join(''));
}
const server=http.createServer((request,response)=>{
 let text='';request.on('data',chunk=>text+=chunk);request.on('end',()=>{
  const body=JSON.parse(text);requests.push(body);fs.writeFileSync(`${lab}/requests.json`,JSON.stringify(requests,null,2));
  if(phase==='bootstrap'){
   phase='bootstrap-result';
   const name='exec';
   const cmd=`'${pkg}/bin/mx-session-start.sh'`;
   toolCalls.push(cmd);stream(response,[{type:'custom_tool_call',name,call_id:'bootstrap-call',input:`text(await tools.exec_command(${JSON.stringify({cmd})}));`}]);
  }else if(phase==='wake-question'||phase==='wake-done'){
   const name='exec';
   const tag=phase;phase=`${tag}-handled`;
   const cmd=`'${binary}' wake claim > '${lab}/${tag}.claim.json'; python3 '${lab}/handle.py' '${lab}/${tag}.claim.json' '${binary}'`;
   toolCalls.push(cmd);stream(response,[{type:'custom_tool_call',name,call_id:tag,input:`text(await tools.exec_command(${JSON.stringify({cmd})}));`}]);
  }else{
   completions.push(phase);stream(response,[{type:'message',role:'assistant',id:`m${requests.length}`,content:[{type:'output_text',text:`fixture ${phase}`}]}]);
  }
 });
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
fs.writeFileSync(`${provider}/config.toml`,`model="gpt-6.1-sol"\nmodel_reasoning_effort="medium"\nmodel_provider="fixture"\n[model_providers.fixture]\nname="Isolated fixture"\nbase_url="http://127.0.0.1:${server.address().port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\n`);
fs.writeFileSync(`${lab}/handle.py`,`import json, subprocess, sys\nitems=[json.loads(line) for line in open(sys.argv[1]) if line.strip()]\nif isinstance(items,dict): items=items.get('items',items.get('events',[]))\nfor item in items:\n event=item['event_id']\n subprocess.run([sys.argv[2],'wake','disposition',event,'handled','--detail','Isolated synthetic model handled child report'],check=True)\n subprocess.run([sys.argv[2],'wake','ack',event],check=True)\n`);
const child=spawn('codex',[...bundle.arguments,'app-server','--enable','codex_hooks','--stdio'],{cwd:pkg,env,stdio:['pipe','pipe','pipe']});
let stderr='';child.stderr.on('data',chunk=>stderr+=chunk);
let id=0;const pending=new Map(),events=[];
readline.createInterface({input:child.stdout}).on('line',line=>{
 let value;try{value=JSON.parse(line);}catch{return;}
 if(value.id!==undefined&&pending.has(value.id)){const p=pending.get(value.id);clearTimeout(p.timer);pending.delete(value.id);value.error?p.reject(Error(JSON.stringify(value.error))):p.resolve(value.result);}else events.push(value);
});
function call(method,params={}){return new Promise((resolve,reject)=>{const requestId=++id;const timer=setTimeout(()=>reject(Error(`RPC timeout ${method}`)),15000);pending.set(requestId,{resolve,reject,timer});child.stdin.write(JSON.stringify({id:requestId,method,params})+'\n');});}
async function wait(test,description){const end=Date.now()+20000;while(Date.now()<end){if(test())return;await new Promise(resolve=>setTimeout(resolve,30));}throw Error(`timeout ${description}`);}
const wakeTurns=()=>events.filter(e=>e.method==='turn/started');
function report(stateName,messageId){const args=['supervision','mx-report','--id','fixture-child','--state',stateName,'--message',stateName==='done'?'Child result completed':'Which fixture choice?','--message-id',messageId];if(stateName==='needs-decision')args.push('--key','choice');if(stateName==='done')args.push('--artifact',result);const reportEnv={...env,MX_TASK_ID:'fixture-child',MX_ATTEMPT_ID:bundle.attempt.id,MX_ATTEMPT_GENERATION:String(bundle.attempt.generation),MX_BRIEF_REVISION:String(bundle.attempt.brief_revision)};const resultRun=spawnSync(binary,args,{env:reportEnv,encoding:'utf8'});assert.equal(resultRun.status,0,resultRun.stderr);return JSON.parse(resultRun.stdout);}
try{
 await call('initialize',{clientInfo:{name:'mx_installed_idle_probe',version:'1'},capabilities:{experimentalApi:true}});child.stdin.write(JSON.stringify({method:'initialized'})+'\n');
 const hooks=(await call('hooks/list',{cwds:[pkg]})).data[0].hooks;
 assert(hooks.some(hook=>hook.eventName==='Stop'||JSON.stringify(hook).includes('mx-turnend-guard.sh')));
 const trusted={};for(const hook of hooks)trusted[hook.key]={trusted_hash:hook.currentHash};
 await call('config/value/write',{keyPath:'hooks.state',value:trusted,mergeStrategy:'replace'});
 const rootThread=(await call('thread/start',{cwd:pkg,approvalPolicy:'never',sandbox:'danger-full-access'})).thread.id;
 const otherThread=(await call('thread/start',{cwd:pkg,approvalPolicy:'never',sandbox:'danger-full-access'})).thread.id;
 await call('turn/start',{threadId:rootThread,input:[{type:'text',text:'Bootstrap isolated root session'}]});
 await wait(()=>completions.includes('bootstrap-result'),'bootstrap completion');
 await wait(()=>fs.existsSync(`${state}/.codex-idle-process.json`),'Stop-owned bridge');
 assert.equal(fs.readFileSync(`${state}/.lock`,'utf8').trim(),String(child.pid));
 const ready=JSON.parse(fs.readFileSync(`${state}/.codex-idle-hook-ready.json`));assert.equal(ready.thread,rootThread);
 const bridge=JSON.parse(fs.readFileSync(`${state}/.codex-idle-process.json`));
 const initialRequests=requests.length;await new Promise(resolve=>setTimeout(resolve,1200));assert.equal(requests.length,initialRequests,'idle model polling');
 phase='wake-question';const questionAt=Date.now();const question=report('needs-decision','fixture-question');assert(question.accepted);
 await wait(()=>completions.includes('wake-question-handled'),'question handling');
 const questionMs=Date.now()-questionAt;
 assert.equal(report('needs-decision','fixture-question').replayed,true);
 const handledRequests=requests.length;await new Promise(resolve=>setTimeout(resolve,1200));assert.equal(requests.length,handledRequests,'duplicate question wake');
 phase='wake-done';const doneAt=Date.now();const done=report('done','fixture-done');assert(done.completion_proven);
 await wait(()=>completions.includes('wake-done-handled'),'done handling');const doneMs=Date.now()-doneAt;
 assert.equal(report('done','fixture-done').replayed,true);
 const finalRequests=requests.length;await new Promise(resolve=>setTimeout(resolve,1200));assert.equal(requests.length,finalRequests,'duplicate done wake');
 assert.equal(events.filter(e=>e.method==='turn/started'&&e.params.threadId===otherThread).length,0,'unrelated thread wake');
 assert.equal(toolCalls.length,3,'only startup and two handling tool calls');
 assert(toolCalls.every(cmd=>!cmd.includes('watch-checkpoint')&&!cmd.includes('watch-arm')),'model supervision polling');
 const unfinished=spawnSync(binary,['wake','list','--unfinished'],{env,encoding:'utf8'});assert.equal(unfinished.status,0,unfinished.stderr);assert.equal(unfinished.stdout.trim(),'','durable unfinished wake loss/handling');
 const evidence={version:spawnSync('codex',['--version'],{encoding:'utf8'}).stdout.trim(),rootThread,otherThread,ready,bridge,questionMs,doneMs,toolCalls,requests:requests.length,turns:wakeTurns().length,events,stderr};
 fs.writeFileSync(`${lab}/evidence.json`,JSON.stringify(evidence,null,2));console.log(JSON.stringify({version:evidence.version,questionMs,doneMs,requests:requests.length,turns:evidence.turns,evidence:`${lab}/evidence.json`}));
}catch(error){fs.writeFileSync(`${lab}/failure.json`,JSON.stringify({error:error.stack,stderr,events,requests,toolCalls},null,2));console.error(error.stack,stderr);process.exitCode=1;}
finally{child.kill('SIGTERM');server.close();}
