// Archived JavaScript implementation.
import { PassThrough } from 'node:stream';
import { emitKeypressEvents } from 'node:readline';
import { clean, rows, details, plain } from './core.mjs';
import { openSource } from './source.mjs';

export const MOUSE_ON = '\x1b[?1000h\x1b[?1006h';
export const MOUSE_OFF = '\x1b[?1000l\x1b[?1006l';

// Keep partial mouse reports between reads. Never send mouse bytes to key handling.
export class MouseDecoder {
  pending = '';
  constructor(mouse, keys) { this.mouse = mouse; this.keys = keys; }
  feed(chunk) {
    this.pending += chunk;
    while (this.pending) {
      const start = this.pending.indexOf('\x1b');
      if (start < 0) { this.keys(this.pending); this.pending = ''; break; }
      if (start > 0) { this.keys(this.pending.slice(0, start)); this.pending = this.pending.slice(start); }
      if (['\x1b', '\x1b['].includes(this.pending)) break;
      if (this.pending.startsWith('\x1b[<')) {
        const match = /^\x1b\[<(\d+);(\d+);(\d+)([Mm])/.exec(this.pending);
        if (!match && this.pending.length < 64 && /^\x1b\[<[\d;]*$/.test(this.pending)) break;
        if (match) {
          this.mouse({ button: Number(match[1]), x: Number(match[2]), y: Number(match[3]), release: match[4] === 'm' });
          this.pending = this.pending.slice(match[0].length);
        } else this.pending = this.pending.slice(3);
      } else { this.keys(this.pending.slice(0, 2)); this.pending = this.pending.slice(2); }
    }
  }
  flush() { if (this.pending && !this.pending.startsWith('\x1b[<')) { this.keys(this.pending); this.pending = ''; } }
}

// ASCII substitution keeps hit regions correct for wide and combining characters.
export const cells = text => clean(text).replace(/[^\x20-\x7e]/gu, '?');

export class TreeView {
  constructor(store, scope, open = openSource, project) {
    Object.assign(this, { store, scope, open, project });
    this.selected = 0; this.offset = 0; this.detailOffset = 0;
    this.folded = new Map(); this.hits = []; this.message = ''; this.lastClick = null;
  }
  action(action) {
    const frame = this.visible?.[this.selected]?.frame;
    this.confirm = action === 'delete' ? this.flow?.name : action === 'confirm' ? this.confirm : undefined;
    try {
      if (action === 'quit') { this.quit?.(); return; }
      if (action === 'open') {
        if (!frame) throw Error('Select a call first.');
        this.open(this.project || this.record?.projectRoot, frame.loc);
        this.message = 'Opened the source in Neovim.';
      }
      if (action === 'confirm' && this.confirm) {
        this.store.delete(this.scope, this.confirm); this.confirm = undefined; this.selected = 0;
        this.message = 'Deleted the selected flow. No undo is available.';
      }
      if (action === 'details') { this.detail = !this.detail; this.detailOffset = 0; }
      if (action === 'fold' && frame?.calls?.length) {
        const set = this.folded.get(this.flow.name), id = this.visible[this.selected].id;
        if (set.has(id)) set.delete(id); else set.add(id);
      }
      if (action === 'expand' && this.flow) this.folded.get(this.flow.name).clear();
      if (action === 'collapse' && this.flow) this.folded.set(this.flow.name, new Set(rows(this.flow).filter(r => r.frame.calls?.length).map(r => r.id)));
      if (['prev', 'next'].includes(action) && this.records.length) {
        const index = this.records.indexOf(this.record);
        this.flowName = this.records[(index + (action === 'next' ? 1 : this.records.length - 1)) % this.records.length].flow.name;
        this.selected = this.offset = this.detailOffset = 0;
      }
      if (['up', 'down', 'pageup', 'pagedown'].includes(action)) {
        const step = action.startsWith('page') ? this.bodySize : 1;
        const delta = action.endsWith('up') ? -step : step;
        if (this.detail) this.detailOffset += delta; else this.selected += delta;
      }
    } catch (e) { this.message = `Error: ${e.message}`; }
  }
  mouse(event, now = Date.now()) {
    if (event.release) return;
    if (event.button === 64 || event.button === 65) { this.action(event.button === 64 ? 'up' : 'down'); return; }
    if (event.button !== 0) return;
    const hit = this.hits.find(h => h.y === event.y && event.x >= h.x && event.x < h.end);
    if (!hit) { this.lastClick = null; return; }
    if (hit.row !== undefined) {
      this.selected = hit.row; this.confirm = undefined;
      if (hit.fold) { this.action('fold'); this.lastClick = null; return; }
      const identity = `${this.flow.name}:${this.visible[hit.row].id}`;
      if (this.lastClick?.identity === identity && now - this.lastClick.time < 400) {
        this.action('open'); this.lastClick = null;
      } else this.lastClick = { identity, time: now };
    } else { this.lastClick = null; this.action(hit.action); }
  }
  render(width, height) {
    width = Math.max(24, width - 1); height = Math.max(12, height);
    this.hits = [];
    try { this.records = this.store.list(this.scope); } catch (e) { this.records = []; this.message = `Error: ${e.message}`; }
    this.record = this.records.find(r => r.flow.name === this.flowName) || this.records[0];
    this.flow = this.record?.flow; this.flowName = this.flow?.name;
    if (!this.folded.has(this.flowName)) this.folded.set(this.flowName, new Set());
    this.visible = this.flow ? rows(this.flow, this.folded.get(this.flowName)) : [];
    this.selected = Math.max(0, Math.min(this.selected, this.visible.length - 1));
    const lines = [`Call stacks | ${this.scope.workspace} | ${this.scope.session}`, this.flow ? `${this.records.indexOf(this.record) + 1}/${this.records.length} ${this.flow.name} [${this.flow.status || 'current'}]` : 'No flows. Ask your agent to publish a flow.'];
    const buttons = this.confirm ? [['Delete this flow?', ''], ['Yes, delete', 'confirm'], ['Cancel', 'cancel']] : [
      ['Prev', 'prev'], ['Next', 'next'], [this.detail ? 'Back' : 'Details', 'details'], ['Open nvim', 'open'], ['Fold', 'fold'], ['Expand all', 'expand'], ['Fold all', 'collapse'], ['Up', 'up'], ['Down', 'down'], ['Page up', 'pageup'], ['Page down', 'pagedown'], ['Delete', 'delete'], ['Quit', 'quit']
    ];
    let line = '';
    for (const [label, action] of buttons) {
      const text = `[${label}]`;
      if (line && line.length + text.length + 1 > width) { lines.push(line); line = ''; }
      if (action) this.hits.push({ x: line.length + 1, end: line.length + text.length + 1, y: lines.length + 1, action });
      line += `${text} `;
    }
    if (line) lines.push(line);
    const startY = lines.length + 1;
    this.bodySize = Math.max(1, height - lines.length - 2);
    const colours = {};
    if (this.detail) {
      const frame = this.visible[this.selected]?.frame;
      const raw = [this.flow?.description || '', frame?.fn || '', frame?.loc || '', ...details(this.flow || {}, frame)];
      const wrapped = raw.flatMap(text => cells(text).match(new RegExp(`.{1,${width}}`, 'g')) || ['']);
      this.detailOffset = Math.max(0, Math.min(this.detailOffset, wrapped.length - this.bodySize));
      lines.push(...wrapped.slice(this.detailOffset, this.detailOffset + this.bodySize));
    } else {
      if (this.selected < this.offset) this.offset = this.selected;
      if (this.selected >= this.offset + this.bodySize) this.offset = this.selected - this.bodySize + 1;
      this.offset = Math.max(0, Math.min(this.offset, Math.max(0, this.visible.length - this.bodySize)));
      this.visible.slice(this.offset, this.offset + this.bodySize).forEach((row, i) => {
        const index = this.offset + i, depth = row.id.split('/').length - 2;
        const prefix = `${index === this.selected ? '>' : ' '} ${'  '.repeat(depth)}`;
        const fold = row.frame.calls?.length ? this.folded.get(this.flowName).has(row.id) ? '[+]' : '[-]' : '   ';
        const marker = { same: '=', added: '+', modified: '~', removed: '-' }[row.change];
        const text = `${prefix}${fold} ${marker} ${row.frame.fn}${row.frame.concurrent ? ' [parallel]' : ''}${row.frame.loc ? `  ${row.frame.loc}` : ''}`;
        // The fold target precedes the row target in hit testing.
        if (row.frame.calls?.length) this.hits.push({ x: prefix.length + 1, end: prefix.length + 4, y: startY + i, row: index, fold: true });
        this.hits.push({ x: 1, end: Math.min(width, cells(text).length) + 1, y: startY + i, row: index });
        colours[lines.length] = { same: 37, added: 32, modified: 33, removed: 31 }[row.change];
        lines.push(text);
      });
    }
    while (lines.length < height - 2) lines.push('');
    lines.push(this.message || '= same | + added | ~ modified | - removed');
    lines.push('Click: select | Double-click: source | Wheel: scroll');
    return lines.slice(0, height).map((text, i) => {
      const value = cells(text).slice(0, width);
      return colours[i] && !process.env.NO_COLOR ? `\x1b[${colours[i]}m${value}\x1b[0m` : value;
    }).join('\r\n');
  }
}

export function view(store, scope, once, project) {
  if (once || !process.stdin.isTTY || !process.stdout.isTTY) {
    console.log(store.list(scope).map(r => plain(r.flow)).join('\n\n') || 'No flows. Publish a flow to start.'); return;
  }
  const model = new TreeView(store, scope, openSource, project), keys = new PassThrough();
  let previous = '', finished = false, flushTimer;
  const draw = () => {
    if (finished) return;
    const output = model.render(process.stdout.columns || 80, process.stdout.rows || 24);
    if (output !== previous) { process.stdout.write(`\x1b[H\x1b[2J${output}`); previous = output; }
  };
  const finish = () => {
    if (finished) return;
    finished = true; clearInterval(timer); clearTimeout(flushTimer);
    process.stdin.setRawMode(false); process.stdin.pause();
    process.stdin.removeListener('data', input); process.stdout.removeListener('resize', draw);
    process.stdout.write(`${MOUSE_OFF}\x1b[?25h\x1b[?1049l`);
    keys.destroy();
  };
  model.quit = finish;
  const decoder = new MouseDecoder(e => { model.mouse(e); draw(); }, text => keys.write(text));
  const input = chunk => { decoder.feed(chunk); clearTimeout(flushTimer); flushTimer = setTimeout(() => decoder.flush(), 40); };
  emitKeypressEvents(keys);
  keys.on('keypress', (_, key = {}) => {
    const actions = { q: 'quit', o: 'open', d: 'details', return: 'fold', space: 'fold', up: 'up', k: 'up', down: 'down', j: 'down', pageup: 'pageup', pagedown: 'pagedown', escape: 'cancel' };
    if (key.ctrl && key.name === 'c') model.action('quit');
    else if (key.name === 'tab') model.action(key.shift ? 'prev' : 'next');
    else if (actions[key.name]) model.action(actions[key.name]);
    draw();
  });
  process.stdin.setEncoding('utf8'); process.stdin.setRawMode(true); process.stdin.resume();
  process.stdout.write(`\x1b[?1049h\x1b[?25l${MOUSE_ON}`);
  const timer = setInterval(draw, 400);
  process.stdin.on('data', input); process.stdout.on('resize', draw);
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.once(signal, () => { finish(); process.exit(0); });
  process.once('exit', finish);
  draw();
}
