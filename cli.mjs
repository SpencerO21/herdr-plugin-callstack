#!/usr/bin/env node
// Compatibility for commands from earlier conversations. The plugin itself does
// not use Node. New commands should run target/release/herdr-callstack directly.
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const binary = fileURLToPath(new URL('./target/release/herdr-callstack', import.meta.url));
const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit' });
child.on('error', error => {
  console.error(`Cannot start the Rust plugin: ${error.message}. Run cargo build --release --locked in the plugin folder.`);
  process.exitCode = 1;
});
child.on('exit', (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
