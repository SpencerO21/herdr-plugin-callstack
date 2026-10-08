// Archived JavaScript tests.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { MouseDecoder, TreeView } from '../terminal.mjs';
import { sourceLocation } from '../source.mjs';

test('mouse decoding handles split reports, release, wheel, and mixed keyboard input', () => {
  const events = [], keys = [];
  const decoder = new MouseDecoder(e => events.push(e), s => keys.push(s));
  for (const part of ['j\x1b', '[<0;12;', '8M\x1b[<0;12;8m', '\x1b[<65;12;8M\x1b[Aq']) decoder.feed(part);
  assert.deepEqual(events, [
    { button: 0, x: 12, y: 8, release: false },
    { button: 0, x: 12, y: 8, release: true },
    { button: 65, x: 12, y: 8, release: false }
  ]);
  assert.equal(keys.join(''), 'j\x1b[Aq');
});

function fixture() {
  let records = ['A', 'B'].map(name => ({ projectRoot: '/project', flow: { name, frames: [{ fn: 'root', loc: 'source.js:12', calls: Array.from({ length: 30 }, (_, i) => ({ fn: `child${i}`, loc: `source.js:${i + 1}` })) }], types: { T: 'x'.repeat(1000) } } }));
  const opened = [];
  const view = new TreeView({ list: () => records, delete: (_, name) => { records = records.filter(r => r.flow.name !== name); } }, { workspace: 'w1', session: 'test' }, (...args) => opened.push(args));
  const render = () => view.render(60, 20);
  const click = (action, time = 1000) => {
    render(); const hit = view.hits.find(h => h.action === action); assert.ok(hit, `Missing button: ${action}`);
    view.mouse({ button: 0, x: hit.x, y: hit.y }, time); render();
  };
  render();
  return { view, opened, render, click };
}
test('all view controls work by mouse, including deletion confirmation', () => {
  const { view, click, render } = fixture();
  click('next'); assert.equal(view.flow.name, 'B');
  click('prev'); assert.equal(view.flow.name, 'A');
  click('collapse'); assert.equal(view.visible.length, 1);
  click('expand'); assert.equal(view.visible.length, 31);
  click('details'); assert.equal(view.detail, true);
  click('pagedown'); assert.ok(view.detailOffset > 0);
  click('details'); assert.equal(view.detail, false);
  view.mouse({ button: 65, x: 1, y: 10 }); render(); assert.equal(view.selected, 1);
  click('open');
  click('delete'); assert.equal(view.records.length, 2);
  click('cancel'); assert.equal(view.records.length, 2);
  click('delete'); click('confirm'); assert.equal(view.records.length, 1);
  let quit = false; view.quit = () => { quit = true; }; click('quit'); assert.ok(quit);
});
test('row click selects, fold click folds, double click opens the source', () => {
  const { view, opened, render } = fixture();
  const fold = view.hits.find(h => h.fold);
  view.mouse({ button: 0, x: fold.x, y: fold.y }); render(); assert.equal(view.visible.length, 1);
  view.mouse({ button: 0, x: fold.x, y: fold.y }); render();
  const row = view.hits.find(h => h.row === 1 && !h.fold);
  view.mouse({ button: 0, x: row.x + 10, y: row.y }, 2000);
  assert.equal(view.selected, 1); assert.equal(opened.length, 0);
  view.mouse({ button: 0, x: row.x + 10, y: row.y }, 2200);
  assert.deepEqual(opened, [['/project', 'source.js:1']]);
});
test('buttons wrap and remain reachable in a narrow pane', () => {
  const { view } = fixture();
  view.render(36, 30);
  for (const hit of view.hits.filter(h => h.action)) {
    assert.ok(hit.x >= 1 && hit.end <= 36);
    assert.ok(hit.y < 30);
  }
});
test('source paths resolve line numbers and reject files outside the project', t => {
  const root = mkdtempSync(join(tmpdir(), 'callstack-source-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const file = join(root, "source ' $(ignored).js"); writeFileSync(file, 'one\ntwo\n');
  const resolved = sourceLocation(root, "source ' $(ignored).js:2:3");
  assert.equal(resolved.line, 2); assert.equal(resolved.file.endsWith("source ' $(ignored).js"), true);
  assert.throws(() => sourceLocation(root, '../missing.js:1'));
  symlinkSync('/etc/hosts', join(root, 'outside.js'));
  assert.throws(() => sourceLocation(root, 'outside.js:1'), /outside/);
  assert.throws(() => sourceLocation(root), /no source/);
  assert.throws(() => sourceLocation(undefined, 'source.js:1'), /project folder/);
});
