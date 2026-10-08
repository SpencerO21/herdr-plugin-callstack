// Archived JavaScript tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync, spawn } from 'node:child_process';
import { Store, scope, validate, rows, plain } from '../core.mjs';

const flow = { name: '../example', status: 'proposed', frames: [{ fn: 'start', change: 'modified', calls: [{ fn: 'child', concurrent: true, in: 'Input' }] }], types: { Input: '{ id: string }' } };
const s = { namespace: 'server-a', workspace: 'w1', session: 'agent-a' };
const cli = resolve(import.meta.dirname, '../cli.mjs');
function temp(t) { const dir = mkdtempSync(join(tmpdir(), 'callstack-test-')); t.after(() => rmSync(dir, { recursive: true, force: true })); return dir; }

test('publish replaces a name and isolates workspaces, servers, and conversations', t => {
  const store = new Store(temp(t));
  store.publish(s, flow);
  store.publish(s, { ...flow, description: 'Updated' });
  assert.equal(store.list(s).length, 1);
  assert.equal(store.list(s)[0].flow.description, 'Updated');
  for (const key of ['namespace', 'workspace', 'session']) assert.deepEqual(store.list({ ...s, [key]: 'other' }), []);
  assert.equal(store.delete(s, flow.name), true);
  assert.equal(store.delete(s, flow.name), false);
});
test('validation rejects invalid nested input', () => {
  for (const f of [{ ...flow, frames: [] }, { ...flow, status: 'wrong' }, { ...flow, frames: [{ fn: 'start', calls: [{ fn: '' }] }] }, { ...flow, types: { Input: 42 } }]) assert.throws(() => validate(f));
  assert.equal(validate(flow), flow);
});
test('tree folds and shows changes, types, and parallel calls', () => {
  assert.equal(rows(flow).length, 2);
  assert.equal(rows(flow, new Set(['/0'])).length, 1);
  assert.match(plain(flow), /~ start/);
  assert.match(plain(flow), /\[parallel\]/);
  assert.match(plain(flow), /type Input/);
  assert.doesNotMatch(plain({ ...flow, name: '\x1b]52;clipboard\x07' }), /[\x1b\x07]/);
});
test('scope requires a workspace and preserves explicit conversation identity', () => {
  assert.throws(() => scope({}, {}));
  assert.deepEqual(scope({ workspace: 'w1', session: 'agent-a', namespace: 'server-a' }, {}), s);
});
test('CLI supports publish, show, view, list, and delete', t => {
  const root = temp(t);
  const run = (args, input) => spawnSync(process.execPath, [cli, ...args, '--workspace', 'test', '--state-dir', root], { encoding: 'utf8', input });
  assert.equal(run(['publish', '-'], JSON.stringify(flow)).status, 0);
  assert.equal(JSON.parse(run(['show', flow.name, '--json']).stdout).flow.name, flow.name);
  assert.match(run(['view', '--once']).stdout, /start/);
  assert.equal(JSON.parse(run(['list', '--json']).stdout).length, 1);
  assert.equal(run(['publish', '-'], '{invalid').status, 1);
  assert.equal(run(['delete', flow.name]).status, 0);
  assert.equal(run(['show', flow.name]).status, 1);
});
test('concurrent writers preserve independent flows', async t => {
  const root = temp(t);
  await Promise.all(Array.from({ length: 12 }, (_, i) => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [cli, 'publish', '-', '--workspace', 'test', '--state-dir', root], { stdio: ['pipe', 'ignore', 'pipe'] });
    child.on('error', reject);
    child.on('exit', code => code === 0 ? resolve() : reject(Error(`Exit ${code}`)));
    child.stdin.end(JSON.stringify({ ...flow, name: `flow-${i}` }));
  })));
  assert.equal(new Store(root).list(scope({ workspace: 'test' })).length, 12);
});
