---
name: call-stack-driven-development
description: Publish current and proposed call flows to the Herdr Call stacks terminal plugin.
---

# Call flow development

A call flow is a tree of function calls with their input and output types.
Use this workflow when the user asks to inspect or change a code path.

1. Read the relevant code. Trace the entry point and its calls.
2. Publish a flow with status `current`. Use verified file locations.
3. For a planned change, publish a separate flow with status `proposed`.
   Mark calls as `same`, `added`, `modified`, or `removed`.
4. After the change and its checks, publish the actual flow with status `current`.
   State which checks ran. Do not imply that a published flow proves correctness.

Find `target/release/herdr-callstack` two directories above this skill directory.
Use its absolute path. This is the Rust executable.
Write a JSON file, then run:

```sh
/absolute/plugin/path/target/release/herdr-callstack publish /absolute/path/flow.json --session TASK
/absolute/plugin/path/target/release/herdr-callstack open --session TASK
```

Use one stable session name for the task. Use the same session for the viewer.
Run publishing commands from the source project, or supply `--project PATH`.
Set valid `loc` fields such as `src/orders.ts:142`. The user can double-click
a call to open that file and line in Neovim.
Inside Herdr, the workspace comes from `HERDR_WORKSPACE_ID`. Outside Herdr,
supply `--workspace NAME` and the viewer's `--namespace NAME`.

A flow needs `name` and `frames`. Each frame needs `fn`. Optional frame fields
are `loc`, `in`, `out`, `cond`, `loop`, `change`, `concurrent`, `module`, `note`,
`detail`, and nested `calls`. Add `types` at the flow level for type definitions.

Keep the main tree to three to five levels where possible. Put supporting
explanations in `detail`. Use `cond` for a condition and `loop` for repeated
calls. Mark calls that start together with `concurrent: true`.

Do not invent file locations or execution order. Label uncertain paths in notes.
Publish the same name to replace a flow. Use distinct names for before and after
views. Do not remove saved flows unless the user asks.
