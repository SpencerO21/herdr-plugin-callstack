// Archived JavaScript implementation.
import { realpathSync, statSync } from 'node:fs';
import { resolve, relative, isAbsolute, sep } from 'node:path';
import { spawnSync } from 'node:child_process';

export function sourceLocation(root, loc) {
  if (!root) throw Error('Set the project folder with --project PATH, then reopen the tree.');
  if (!loc) throw Error('This call has no source location.');
  const match = /^(.*?):([1-9][0-9]*)(?::([1-9][0-9]*))?$/.exec(loc);
  const path = match ? match[1] : loc;
  if (/[\x00-\x1f\x7f]/.test(path)) throw Error('The source path contains a control character.');
  const project = realpathSync(root), file = realpathSync(resolve(project, path));
  const rel = relative(project, file);
  if (rel === '..' || rel.startsWith(`..${sep}`) || isAbsolute(rel)) throw Error('The source file is outside the project folder.');
  if (!statSync(file).isFile()) throw Error('The source path is not a file.');
  const line = Number(match?.[2] || 1);
  if (!Number.isSafeInteger(line)) throw Error('The source line is invalid.');
  return { project, file, line };
}

export function herdr(args) {
  const result = spawnSync(process.env.HERDR_BIN_PATH || 'herdr', args, { encoding: 'utf8', timeout: 10000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(result.stderr || 'The Herdr command failed.');
  return JSON.parse(result.stdout).result;
}

export function openSource(project, loc) {
  const source = sourceLocation(project, loc);
  const target = process.env.HERDR_PANE_ID;
  if (!target) throw Error('Open the tree in a Herdr pane to use Neovim.');
  const args = ['plugin', 'pane', 'open', '--plugin', 'callstack', '--entrypoint', 'editor', '--target-pane', target, '--direction', 'right', '--focus'];
  for (const [key, value] of Object.entries({ CALLSTACK_PROJECT: source.project, CALLSTACK_FILE: source.file, CALLSTACK_LINE: String(source.line) })) args.push('--env', `${key}=${value}`);
  herdr(args);
}

export function editSource() {
  const source = sourceLocation(process.env.CALLSTACK_PROJECT, `${process.env.CALLSTACK_FILE || ''}:${process.env.CALLSTACK_LINE || '1'}`);
  // Use argv, not a shell command. File names cannot become commands.
  const result = spawnSync('nvim', [`+${source.line}`, '--', source.file], { cwd: source.project, stdio: 'inherit' });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}
