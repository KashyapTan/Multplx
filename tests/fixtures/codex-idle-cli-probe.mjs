// Actual CLI hooks/queue with an isolated synthetic Responses endpoint.
// Usage: node codex-idle-cli-probe.mjs ABS_MX ABS_REPOSITORY
// This never reads auth material or writes an existing CODEX_HOME.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import { spawn } from 'node:child_process';
import readline from 'node:readline';

const [mx, repo] = process.argv.slice(2);
assert(mx?.startsWith('/') && repo?.startsWith('/'), 'absolute mx and repository paths required');
const root = fs.mkdtempSync('/tmp/mx-codex-hook-research.');
console.log(`fixture ${root}`);
for (const name of ['home', 'work', 'state', 'bin']) fs.mkdirSync(`${root}/${name}`);
fs.writeFileSync(`${root}/.mx-daemon-home`, 'isolated-cli-proof\n');
fs.writeFileSync(`${root}/AGENTS.md`, '# isolated CLI fixture\n');

const requests = [];
let releaseHeld;
let holdNext = false;
function finish(response, number) {
  const items = [
    { type: 'response.created', response: { id: `r${number}` } },
    { type: 'response.output_item.done', item: {
      type: 'message', role: 'assistant', id: `m${number}`,
      content: [{ type: 'output_text', text: `fixture reply ${number}` }],
    } },
    { type: 'response.completed', response: {
      id: `r${number}`, usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 },
    } },
  ];
  response.writeHead(200, { 'Content-Type': 'text/event-stream' });
  response.end(items.map(item => `data: ${JSON.stringify(item)}\n\n`).join(''));
}
const server = http.createServer((request, response) => {
  let body = '';
  request.on('data', chunk => { body += chunk; });
  request.on('end', () => {
    requests.push(JSON.parse(body));
    if (holdNext) {
      holdNext = false;
      releaseHeld = () => finish(response, requests.length);
    } else finish(response, requests.length);
  });
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
fs.writeFileSync(`${root}/home/config.toml`, `
model = "gpt-6.1-sol"
model_reasoning_effort = "medium"
model_provider = "fixture"
[model_providers.fixture]
name = "Local research fixture"
base_url = "http://127.0.0.1:${server.address().port}/v1"
wire_api = "responses"
requires_openai_auth = false
`);
fs.writeFileSync(`${root}/hook.py`, `import json,sys
from pathlib import Path
payload=json.load(sys.stdin)
with Path("${root}/hook-payloads.jsonl").open("a") as output:
    output.write(json.dumps(payload)+"\\n")
print("{}")
`);
const registration = JSON.parse(fs.readFileSync(`${repo}/.codex/hooks.json`, 'utf8'))
  .hooks.SessionStart[0].hooks.find(hook => hook.command.includes('mx-codex-idle.sh'));
assert(registration, 'tracked SessionStart registration required');
fs.writeFileSync(`${root}/home/hooks.json`, JSON.stringify({ hooks: {
  SessionStart: [{ hooks: [registration] }],
  Stop: [{ hooks: [{ type: 'command', command: `python3 ${root}/hook.py`, timeout: 5 }] }],
} }));
const env = {
  ...process.env, CODEX_HOME: `${root}/home`, MX_ROOT_OVERRIDE: root, MX_HOME: root,
  MX_STATE_OVERRIDE: `${root}/state`, MX_RUST_SOURCE_ROOT: repo, MX_RUST_BIN: mx,
  MX_CODEX_IDLE_CLI: '1',
};
for (const key of ['MX_TASK_ID', 'MX_CURRENT_ADMISSION_ID', 'MX_SHIM_DIR', 'MX_REAL_CODEX', 'DEEP_REVIEW_GATE']) {
  delete env[key];
}
const child = spawn('codex', ['app-server', '--enable', 'codex_hooks', '--stdio'], {
  env, cwd: `${root}/work`, stdio: ['pipe', 'pipe', 'pipe'],
});
let stderr = '';
child.stderr.on('data', chunk => { stderr += chunk; });
let id = 0;
const pending = new Map();
const events = [];
readline.createInterface({ input: child.stdout }).on('line', line => {
  let value;
  try { value = JSON.parse(line); } catch { return; }
  if (value.id !== undefined && pending.has(value.id)) {
    const { resolve, reject, timer } = pending.get(value.id);
    clearTimeout(timer);
    pending.delete(value.id);
    if (value.error) reject(Error(JSON.stringify(value.error)));
    else resolve(value.result);
  } else events.push(value);
});
function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const requestId = ++id;
    const timer = setTimeout(() => {
      pending.delete(requestId);
      reject(Error(`RPC timeout ${method}`));
    }, 15000);
    pending.set(requestId, { resolve, reject, timer });
    child.stdin.write(`${JSON.stringify({ id: requestId, method, params })}\n`);
  });
}
async function wait(predicate) {
  const until = Date.now() + 15000;
  while (Date.now() < until) {
    const found = events.find(predicate);
    if (found) return found;
    await new Promise(resolve => setTimeout(resolve, 30));
  }
  throw Error('event timeout');
}
function queue(thread, message) {
  return new Promise((resolve, reject) => {
    const process = spawn('codex', ['queue', '--thread', thread, '--message', message], {
      env, cwd: `${root}/work`,
    });
    const timer = setTimeout(() => {
      process.kill('SIGTERM');
      reject(Error('queue timeout'));
    }, 20000);
    let output = '';
    let error = '';
    process.stdout.on('data', chunk => { output += chunk; });
    process.stderr.on('data', chunk => { error += chunk; });
    process.on('exit', code => {
      clearTimeout(timer);
      if (code === 0) resolve(output.trim());
      else reject(Error(`queue failed ${code}: ${error}`));
    });
  });
}
const inputStarted = text => event => event.method === 'item/started' && JSON.stringify(event).includes(text);
const completed = turn => event => event.method === 'turn/completed' && event.params.turn.id === turn;
try {
  await call('initialize', {
    clientInfo: { name: 'mx_queue_research', version: '1' }, capabilities: { experimentalApi: true },
  });
  child.stdin.write(`${JSON.stringify({ method: 'initialized' })}\n`);
  const listing = await call('hooks/list', { cwds: [`${root}/work`] });
  // Review only the just-created isolated fixture hook hashes, never a real home.
  const trusted = {};
  for (const hook of listing.data[0].hooks) trusted[hook.key] = { trusted_hash: hook.currentHash };
  await call('config/value/write', { keyPath: 'hooks.state', value: trusted, mergeStrategy: 'replace' });
  const startParams = { cwd: `${root}/work`, approvalPolicy: 'never', sandbox: 'read-only' };
  const a = (await call('thread/start', startParams)).thread.id;
  const b = (await call('thread/start', startParams)).thread.id;
  await call('turn/start', { threadId: a, input: [{ type: 'text', text: 'materialize A' }] });
  await wait(event => event.method === 'turn/completed' && event.params.threadId === a);
  const readiness = JSON.parse(fs.readFileSync(`${root}/state/.codex-idle-hook-ready.json`, 'utf8'));
  assert.equal(readiness.thread, a);
  assert.equal(readiness.owner.pid, child.pid);
  assert(!fs.existsSync(`${root}/state/.lock`), 'bootstrap must not preseed lock');
  fs.writeFileSync(`${root}/state/.lock`, String(child.pid));

  const queuedAt = Date.now();
  await queue(a, 'IDLE-WAKE-A');
  const idle = await wait(inputStarted('IDLE-WAKE-A'));
  assert.equal(idle.params.threadId, a);
  await wait(completed(idle.params.turnId));
  const idleWakeMs = Date.now() - queuedAt;
  const humanAt = Date.now();
  await call('turn/start', { threadId: a, input: [{ type: 'text', text: 'HUMAN-IDLE-PROMPT' }] });
  const human = await wait(inputStarted('HUMAN-IDLE-PROMPT'));
  const idleHumanPromptMs = Date.now() - humanAt;
  assert(idleHumanPromptMs < 2000, 'idle human input must remain responsive');
  await wait(completed(human.params.turnId));

  holdNext = true;
  const busy = (await call('turn/start', { threadId: a, input: [{ type: 'text', text: 'BUSY-A' }] })).turn.id;
  await new Promise(resolve => setTimeout(resolve, 150));
  await queue(a, 'BUSY-QUEUED-A');
  await new Promise(resolve => setTimeout(resolve, 400));
  assert(!events.some(inputStarted('BUSY-QUEUED-A')), 'busy queued input started prematurely');
  assert(releaseHeld, 'busy model response must be held');
  releaseHeld();
  const queued = await wait(inputStarted('BUSY-QUEUED-A'));
  await wait(completed(queued.params.turnId));
  const busyTurnCompletedIndex = events.findIndex(completed(busy));
  const queuedItemStartedIndex = events.findIndex(inputStarted('BUSY-QUEUED-A'));
  assert(busyTurnCompletedIndex >= 0 && busyTurnCompletedIndex < queuedItemStartedIndex);
  const wrongThreadEvents = events.filter(event => event.method === 'turn/started' && event.params.threadId === b).length;
  assert.equal(wrongThreadEvents, 0);
  const payloads = fs.readFileSync(`${root}/hook-payloads.jsonl`, 'utf8').trim().split('\n').map(JSON.parse);
  assert.equal(payloads.length, 5);
  assert(payloads.every(payload => payload.session_id === a && payload.stop_hook_active === false));
  assert.equal(new Set(payloads.map(payload => payload.turn_id)).size, 5);
  const result = { idleWakeMs, idleHumanPromptMs, busyTurnCompletedIndex, queuedItemStartedIndex, wrongThreadEvents, requests: requests.length };
  console.log(JSON.stringify(result));
  fs.writeFileSync(`${root}/evidence.json`, JSON.stringify({ a, b, readiness, result, events, requests, stderr }, null, 2));
} catch (error) {
  console.error(error.stack);
  console.error(stderr);
  process.exitCode = 1;
} finally {
  child.kill('SIGTERM');
  server.close();
}
