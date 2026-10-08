# Archived JavaScript version

This is the previous version. The active plugin uses Rust. See ../README.md.

Inspect agent-supplied call flows in a live terminal pane. A call flow is a tree
of function calls. Each call can show its source location, types, conditions,
and change status. The plugin does not trace a running program.

Requires Node.js 22 or later and Herdr 0.9.1 or later. Source opening also requires
`nvim` on PATH. No package install is needed.

## Install locally

```sh
herdr plugin link /Users/spencer/projects/herdr-plugin-callstack
```

Select **Call stacks: open** from the Herdr plugin actions. Or run this command
from a Herdr terminal:

```sh
node /Users/spencer/projects/herdr-plugin-callstack/cli.mjs open
```

## Publish a flow

Write a JSON file with `name`, `status`, and `frames`. See
[the example](examples/order.json). Publish it from the agent's Herdr terminal:

```sh
node /Users/spencer/projects/herdr-plugin-callstack/cli.mjs publish flow.json
```

Publish the same name to replace a flow. Different names remain separate.
`publish -` reads JSON from standard input.

```sh
node /Users/spencer/projects/herdr-plugin-callstack/cli.mjs list
node /Users/spencer/projects/herdr-plugin-callstack/cli.mjs show 'Submit order'
node /Users/spencer/projects/herdr-plugin-callstack/cli.mjs delete 'Submit order'
```

Use `--json` with `list` or `show` for structured output. Use `view --once` for
plain text. `delete` removes only the named flow. It has no undo command.

## Controls

All view controls have mouse buttons. Click a call to select it. Click its
`[+]` or `[-]` control to fold or expand its calls. Double-click the function
row, or click **Open nvim**, to open its source in a new Neovim pane at the
specified line. Use the mouse wheel or the page buttons to scroll.

Use **Prev** and **Next** to switch flows. Use **Details** to read types and notes.
Use **Back** to return to the tree. **Expand all**, **Fold all**, and **Quit**
also have buttons. **Delete** asks for a second click before it removes a flow.
The pane restores normal terminal mouse handling when it closes.

Publishing flow documents remains an agent or command-line operation.
Restart an existing view after updating the plugin to load the new controls.

| Key | Action |
| --- | --- |
| Up/Down or j/k | Select a call |
| Enter or Space | Fold or expand its calls |
| Tab / Shift+Tab | Select the next or previous flow |
| d | Show or close call details and type definitions |
| o | Open the selected source location in Neovim |
| PageUp / PageDown | Move by one page |
| q / Ctrl+C | Close the view |

The view checks for changes every 400 milliseconds. Change labels use both text
and colour: `=` same, `+` added, `~` modified, and `-` removed. Set `NO_COLOR=1`
to disable colour. `[parallel]` marks calls that can run at the same time.

## Source folder

Use `--project PATH` with `open` to select the project folder explicitly:

```sh
node /Users/spencer/projects/herdr-plugin-callstack/cli.mjs open --project /path/to/project
```

Otherwise, `open` uses the calling pane's folder. New published flows also store
the publishing command's folder. A direct `view` can use this stored folder.
Pass `--project PATH` to `publish` if you run it outside the source project.

Locations use `relative/file.ts:142` or `relative/file.ts:142:5`. Neovim opens
at the line; the optional column is not used. A missing file or location shows
an error in the tree. The plugin rejects paths that resolve outside the project
folder, including symbolic links. It opens each request in a new editor pane.

## Scope and storage

The store separates flows by server namespace, workspace, and agent session.
The server namespace defaults to `HERDR_SOCKET_PATH`, or `local` if absent.
The workspace defaults to `HERDR_WORKSPACE_ID`. Outside Herdr, supply
`--workspace NAME`. Use the same `--namespace NAME` for both commands and the
viewer when publishing from another process without the same Herdr environment.

The agent session defaults to `default`. For separate conversations, pass the
same `--session NAME` to both `publish` and `open`. You can also set
`CALLSTACK_SESSION`. The plugin does not infer agent conversation identifiers.

Data lives in `$XDG_STATE_HOME/herdr-callstack`, or
`~/.local/state/herdr-callstack`. This shared path lets direct agent commands and
plugin panes use the same store. Set `CALLSTACK_STATE_DIR` or `--state-dir PATH`
to override it. The `open` command passes the resolved scope and storage path
to its pane. Removing the plugin does not remove saved flows.

Each flow has its own file. Writes use a temporary file and an atomic rename.
For two writes to the same flow, the last completed rename wins.

## Agent instructions

Read [the bundled skill](skills/call-stack-driven-development/SKILL.md) in your
agent conversation. Linking a Herdr plugin does not install skills into agents.
Use an absolute path to `cli.mjs` when the agent works in another repository.

## Format and limits

The flow format follows the documented fields of
[bb-plugin-callstack](https://github.com/ebg1223/bb-plugin-callstack).
This is an independent implementation. It does not use BB runtime code.

Flow fields: `name`, `description`, `status` (`current` or `proposed`), `frames`,
and `types` (a map from type names to definitions).

Frame fields: `fn`, `loc`, `in`, `out`, `cond`, `loop`, `change`, `concurrent`,
`module`, `note`, `detail`, and nested `calls`. `fn` is required. `change` is
`same`, `added`, `modified`, or `removed`.

Limits: 2 MB of input, 50 calls per group, 20 nested levels, and 5000 total calls.
The viewer replaces terminal control characters in supplied text.

This release does not detect source changes, archive flows, or capture runtime
calls. The agent must update its flows after code changes.

## Check

```sh
npm test
```
