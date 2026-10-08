#!/usr/bin/env node
// Archived JavaScript implementation. Herdr now runs the Rust executable.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { Store, scope, plain, clean } from './core.mjs';
import { view } from './terminal.mjs';
import { herdr, editSource } from './source.mjs';

const HELP = `Call stacks for Herdr
  node cli.mjs publish FILE|- [--workspace NAME] [--session NAME]
  node cli.mjs list [--json]
  node cli.mjs show NAME [--json]
  node cli.mjs delete NAME
  node cli.mjs view [--once]
  node cli.mjs open [--project PATH]
Options: --workspace NAME --session NAME --namespace NAME --state-dir PATH
         --project PATH (the source folder; defaults to the caller folder)
Mouse: click a call to select it; double-click to open its source in Neovim.
       Click [+]/[-] to fold. Use the buttons for all other view controls.
Keys: arrows or j/k select; Enter folds; Tab changes flow; d shows details;
      o opens Neovim; PageUp/PageDown scroll; q closes the view.
Set CALLSTACK_SESSION to keep agent conversations separate.
Set CALLSTACK_STATE_DIR to use a separate data directory.`;

function parse(args) {
  const options = {}, values = [];
  for (let i = 0; i < args.length; i++) {
    if (!args[i].startsWith('--')) { values.push(args[i]); continue; }
    const key = args[i].slice(2);
    if (['json', 'once', 'help'].includes(key)) options[key] = true;
    else if (['workspace', 'session', 'namespace', 'state-dir', 'project'].includes(key)) {
      if (!args[i + 1] || args[i + 1].startsWith('--')) throw Error(`Missing value for --${key}.`);
      options[key] = args[++i];
    } else throw Error(`Unknown option: --${key}`);
  }
  return { options, values };
}

try {
  const { options, values } = parse(process.argv.slice(2));
  const [command, name, ...extra] = values;
  if (!command || command === 'help' || options.help) console.log(HELP);
  else if (command === 'edit') editSource();
  else {
    if (extra.length || (['list', 'view', 'open'].includes(command) && name)) throw Error('Unexpected arguments. Use --help.');
    const s = scope(options), store = new Store(options['state-dir']);
    const explicitProject = options.project || process.env.CALLSTACK_PROJECT;
    if (command === 'publish') {
      if (!name) throw Error('Supply a JSON file or - for standard input.');
      const input = readFileSync(name === '-' ? 0 : name, 'utf8');
      if (Buffer.byteLength(input) > 2_000_000) throw Error('The flow exceeds 2 MB.');
      const record = store.publish(s, JSON.parse(input), resolve(explicitProject || process.cwd()));
      console.log(`Published: ${clean(record.flow.name)}`);
    } else if (command === 'list') {
      const records = store.list(s);
      console.log(options.json ? JSON.stringify(records, null, 2) : records.map(r => `${clean(r.flow.name)} [${r.flow.status || 'current'}]`).join('\n') || 'No flows.');
    } else if (command === 'show') {
      const record = store.list(s).find(r => r.flow.name === name);
      if (!record) throw Error('Flow not found.');
      console.log(options.json ? JSON.stringify(record, null, 2) : plain(record.flow));
    } else if (command === 'delete') {
      if (!name) throw Error('Supply a flow name.');
      if (!store.delete(s, name)) throw Error('Flow not found.');
      console.log(`Deleted: ${clean(name)}`);
    } else if (command === 'view') view(store, s, options.once, explicitProject && resolve(explicitProject));
    else if (command === 'open') {
      const pane = herdr(['pane', 'current', '--current']).pane;
      if (pane.workspace_id !== s.workspace) throw Error('Run open from the requested workspace.');
      const project = resolve(explicitProject || pane.foreground_cwd || pane.cwd);
      const args = ['plugin', 'pane', 'open', '--plugin', 'callstack', '--entrypoint', 'tree', '--target-pane', pane.pane_id, '--direction', 'right', '--no-focus'];
      for (const [k,v] of Object.entries({ CALLSTACK_NAMESPACE: s.namespace, CALLSTACK_WORKSPACE: s.workspace, CALLSTACK_SESSION: s.session, CALLSTACK_STATE_DIR: store.root, CALLSTACK_PROJECT: project })) args.push('--env', `${k}=${v}`);
      console.log(JSON.stringify(herdr(args)));
    } else throw Error(`Unknown command: ${command}`);
  }
} catch (e) { console.error(`Error: ${clean(e.message)}`); process.exitCode = 1; }
