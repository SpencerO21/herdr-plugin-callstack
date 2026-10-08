// Archived JavaScript implementation.
import { createHash, randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, renameSync, writeFileSync, unlinkSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const clean = value => String(value).replace(/[\x00-\x1f\x7f-\x9f]/g, ' ');
const hash = value => createHash('sha256').update(value).digest('hex');
export function validate(flow) {
  const object = (v, path) => {
    if (!v || typeof v !== 'object' || Array.isArray(v)) throw Error(`${path} must be an object.`);
  };
  const string = (v, path, max, required = false) => {
    if (v === undefined && !required) return;
    if (typeof v !== 'string' || v.length > max || (required && !v.trim())) throw Error(`${path} must be text of length ${required ? 1 : 0}–${max}.`);
  };
  const choice = (v, path, choices) => {
    if (v !== undefined && !choices.includes(v)) throw Error(`${path} must be one of: ${choices.join(', ')}.`);
  };
  object(flow, 'flow');
  string(flow.name, 'name', 120, true);
  string(flow.description, 'description', 1000);
  choice(flow.status, 'status', ['current', 'proposed']);
  let count = 0;
  function frames(items, depth, path) {
    if (!Array.isArray(items) || items.length > 50 || (depth === 0 && !items.length)) throw Error(`${path} must contain ${depth === 0 ? '1–50' : '0–50'} frames.`);
    if (depth > 20) throw Error('The flow exceeds 20 call levels.');
    for (const [i, frame] of items.entries()) {
      if (++count > 5000) throw Error('The flow exceeds 5000 frames.');
      const p = `${path}[${i}]`;
      object(frame, p);
      string(frame.fn, `${p}.fn`, 200, true);
      for (const k of ['loc', 'in', 'out', 'cond', 'loop']) string(frame[k], `${p}.${k}`, 300);
      string(frame.module, `${p}.module`, 60, frame.module !== undefined);
      string(frame.note, `${p}.note`, 500);
      string(frame.detail, `${p}.detail`, 2000);
      choice(frame.change, `${p}.change`, ['same', 'added', 'modified', 'removed']);
      if (frame.concurrent !== undefined && typeof frame.concurrent !== 'boolean') throw Error(`${p}.concurrent must be true or false.`);
      if (frame.calls !== undefined) frames(frame.calls, depth + 1, `${p}.calls`);
    }
  }
  frames(flow.frames, 0, 'frames');
  if (flow.types !== undefined) {
    object(flow.types, 'types');
    for (const [key, value] of Object.entries(flow.types)) {
      string(key, 'type name', 200, true);
      string(value, `types.${key}`, 2000, true);
    }
  }
  return flow;
}

export function scope(options = {}, env = process.env) {
  const workspace = options.workspace || env.CALLSTACK_WORKSPACE || env.HERDR_WORKSPACE_ID;
  if (!workspace) throw Error('Set --workspace NAME outside a Herdr pane.');
  // Herdr workspace ids repeat across servers. Include the socket or an explicit namespace.
  return { namespace: options.namespace || env.CALLSTACK_NAMESPACE || env.HERDR_SOCKET_PATH || 'local', workspace, session: options.session || env.CALLSTACK_SESSION || 'default' };
}
export class Store {
  constructor(root = process.env.CALLSTACK_STATE_DIR || join(process.env.XDG_STATE_HOME || join(homedir(), '.local', 'state'), 'herdr-callstack')) { this.root = root; }
  directory(s) { return join(this.root, hash(JSON.stringify([s.namespace, s.workspace, s.session]))); }
  file(s, name) { return join(this.directory(s), `${hash(name)}.json`); }
  publish(s, flow, projectRoot) {
    validate(flow);
    mkdirSync(this.directory(s), { recursive: true, mode: 0o700 });
    const record = { flow, scope: s, projectRoot, updatedAt: new Date().toISOString() };
    const file = this.file(s, flow.name);
    const temp = `${file}.${randomUUID()}.tmp`;
    try {
      writeFileSync(temp, JSON.stringify(record, null, 2), { mode: 0o600, flag: 'wx' });
      renameSync(temp, file);
    } finally { try { unlinkSync(temp); } catch (e) { if (e.code !== 'ENOENT') throw e; } }
    return record;
  }
  list(s) {
    let files;
    try { files = readdirSync(this.directory(s)); } catch (e) { if (e.code === 'ENOENT') return []; throw e; }
    return files.filter(f => f.endsWith('.json')).flatMap(f => {
      try {
        const record = JSON.parse(readFileSync(join(this.directory(s), f), 'utf8'));
        validate(record.flow);
        return [record];
      } catch (e) { if (e.code === 'ENOENT') return []; throw Error(`Cannot read stored flow ${f}: ${e.message}`); }
    }).sort((a, b) => a.flow.name.localeCompare(b.flow.name));
  }
  delete(s, name) {
    try { unlinkSync(this.file(s, name)); return true; } catch (e) { if (e.code === 'ENOENT') return false; throw e; }
  }
}

export function rows(flow, collapsed = new Set()) {
  const result = [];
  function walk(frames, depth, parent) {
    frames.forEach((frame, i) => {
      const id = `${parent}/${i}`;
      const change = frame.change || 'same';
      const mark = { same: '=', added: '+', modified: '~', removed: '-' }[change];
      result.push({ id, frame, change, text: `${'  '.repeat(depth)}${frame.calls?.length ? collapsed.has(id) ? '▸' : '▾' : '·'} ${mark} ${frame.fn}${frame.concurrent ? ' [parallel]' : ''}${frame.loc ? `  ${frame.loc}` : ''}` });
      if (frame.calls?.length && !collapsed.has(id)) walk(frame.calls, depth + 1, id);
    });
  }
  walk(flow.frames, 0, '');
  return result;
}
export function details(flow, frame) {
  if (!frame) return [];
  const lines = [];
  for (const k of ['in', 'out', 'cond', 'loop', 'module', 'note', 'detail']) if (frame[k]) lines.push(`${k}: ${frame[k]}`);
  if (flow.types) for (const [name, definition] of Object.entries(flow.types)) lines.push(`type ${name}: ${definition}`);
  return lines;
}
export function plain(flow) {
  return [flow.name + ` [${flow.status || 'current'}]`, flow.description || '', ...rows(flow).flatMap(row => [row.text, ...details({ ...flow, types: undefined }, row.frame).map(s => `    ${s}`)]), ...Object.entries(flow.types || {}).map(([k,v]) => `type ${k}: ${v}`)].map(clean).join('\n');
}
