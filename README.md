# Call stacks for Herdr

A Rust plugin that shows agent-supplied call flows in a terminal pane.
A call flow is a tree of function calls. Each call can show its source location,
types, conditions, and change label. The plugin does not record a running program.

## Build and install

Requires Rust 1.88 or later to build. Herdr 0.9.1 or later runs the plugin.
Source opening also requires `nvim` on PATH. Supported systems: macOS and Linux.
The running plugin does not require Node.js.

```sh
cd /Users/spencer/projects/herdr-plugin-callstack
cargo build --release --locked
herdr plugin link /Users/spencer/projects/herdr-plugin-callstack
```

Herdr runs `target/release/herdr-callstack` directly. The manifest also defines
the build command for a future GitHub install. A local `plugin link` does not
build it. Rebuild after source changes, then reopen existing views.

Select **Call stacks: open** from the Herdr plugin actions. Or run this command
from a Herdr terminal:

```sh
/Users/spencer/projects/herdr-plugin-callstack/target/release/herdr-callstack open
```

## Publish and inspect flows

The agent reads the code and creates a JSON flow file. Publishing saves it
locally and makes it visible in the pane. Nothing is uploaded.

Run commands from the source project, or supply `--project /path/to/project`:

```sh
/Users/spencer/projects/herdr-plugin-callstack/target/release/herdr-callstack publish flow.json --session my-task
/Users/spencer/projects/herdr-plugin-callstack/target/release/herdr-callstack open --session my-task
```

Use the same session for both commands. Publishing the same name replaces its
flow. Different names remain separate. `publish -` reads standard input.

Other commands are `list`, `show NAME`, `delete NAME`, and `view --once`.
`list --json` and `show NAME --json` return structured data.
`delete` removes only the named flow. It has no undo command.

Ask an agent to read [the bundled skill](skills/call-stack-driven-development/SKILL.md)
to use this workflow. Linking the plugin does not install that skill into agents.
Publishing remains an agent or command-line operation. There is no agent-start
button or agent-refresh shortcut in this release.

## Mouse and keyboard

| Mouse control | Keyboard | Result |
| --- | --- | --- |
| Click a function | Up/Down or j/k | Select a call |
| Click `[+]` or `[-]`, or Fold | Enter or Space | Expand or fold nested calls |
| Double-click a function, or Open nvim | o | Open the source line in a new Neovim pane |
| Diff dn | g | Open the selected file's changes in DiffNav through `dn` |
| Prev / Next | Shift+Tab / Tab | Change flows |
| Details / Back | d | Show types, conditions, and notes |
| Wheel, Up/Down, Page up/down | Arrows, PageUp/PageDown | Scroll |
| Left / Right | Left/Right | Read long tree rows |
| Expand all / Fold all | — | Change the whole tree |
| Delete, then Yes, delete | — | Remove the selected flow |
| Cancel | Escape | Cancel deletion |
| Quit | q or Ctrl+C | Close the view |

The legend stays visible above the status line:
`=` same, `+` added, `~` modified, and `-` removed.
The boxed `[+]` and `[-]` controls fold calls; they are not change labels.
Set `NO_COLOR=1` to disable colours. `[parallel]` marks calls that can start together.

Buttons wrap in narrow panes. The full view needs at least 29 columns and 16 rows.
Smaller panes show a resize message and a mouse-accessible Quit button.
Terminal mouse handling and the normal screen are restored on exit.

## Source locations

`open` uses the calling pane's source folder. Override it with `--project PATH`.
`publish` stores its source folder with the flow. A direct `view` uses that stored
folder if no explicit project was supplied.

Locations use `relative/file.rs:142` or `relative/file.rs:142:5`. Neovim opens
at the line; the optional column is not used. A missing location or file shows
an error. Paths outside the project folder are rejected, including symbolic links.
File names are passed as separate arguments, not as shell commands.

Each source-open request creates a new editor pane. Herdr commands run on a
worker with a 10-second timeout. You can still navigate or quit while a command
runs. A second open request is ignored until the first one finishes.
The editor process replaces itself with Neovim; it leaves no wrapper process.

## Source checks and change warnings

Publishing checks source file locations, line numbers, and function names.
It also warns about inconsistent status/change labels, unused type definitions,
parallel calls without a parallel neighbour, and near-matching function names
across saved flows. Warnings do not block publication. Function checks search
text; they do not prove that a function calls another function.

The plugin saves a content hash for each referenced source file. It watches
their parent folders and marks affected calls with **SOURCE CHANGED** when file
contents differ. Deleted files also cause warnings. Open **Details** to read
the warnings and changed-file list. Republish a verified flow to set a new
baseline. Restoring the exact saved file content also clears the change marker.

Source checks run outside the input loop. Unchanged source files reuse cached
hashes. The five-second metadata check recovers missed file events. Files outside
the project, non-text files, and files over 4 MB cannot be checked.
Old records show **UNTRACKED** until republished. Source tracking always uses
the project folder saved at publication. It does not use a different viewer
`--project` override. The plugin does not generate a corrected tree or move
source line numbers automatically.

## DiffNav

Click **Diff dn** or press `g` to open the selected file's diff in another pane.
The pane runs your interactive shell's `dn` command with a file-specific
`--watch-cmd`. Define `dn` as an alias or function for DiffNav, for example:

```sh
alias dn='diffnav --watch'
```

The diff compares the working tree with the common ancestor of `origin/main`
and `HEAD`. This includes branch changes and local edits. If `origin/main` is
unavailable, it compares with `HEAD`. It shows only the selected file, including
deleted files. Untracked files are not included by Git diff.
DiffNav has no source-line jump option, so this opens the file's changes, not
an exact function line. Your existing alias's watch command is overridden for
this pane only. Your shell configuration is not changed.

## Storage and compatibility

Flows are separated by server namespace, workspace, and agent session.
The namespace defaults to `HERDR_SOCKET_PATH`, or `local` if absent.
The workspace defaults to `HERDR_WORKSPACE_ID`. Outside Herdr, supply
`--workspace NAME`. Supply `--namespace NAME` when the publisher does not have
the same Herdr environment as the viewer.

The session defaults to `default`. Set `--session NAME` or `CALLSTACK_SESSION`
to separate agent tasks. The plugin does not infer conversation identifiers.

Data lives in `$XDG_STATE_HOME/herdr-callstack`, or
`~/.local/state/herdr-callstack`. Set `CALLSTACK_STATE_DIR` or `--state-dir PATH`
to change it. Use an absolute path. The open command passes its resolved scope
and paths to the viewer.

The Rust version reads the previous JavaScript records and uses the same hashed
file paths. No migration is needed. Each flow has its own file. Publishing writes
and syncs a temporary file, then renames it atomically. The last completed write
to a given flow wins. Uninstalling the plugin does not delete flow data.

Old `node cli.mjs …` commands still work through a small forwarding script.
The previous implementation is kept in `legacy/` for comparison. Herdr does not
run it. New automation should use the Rust executable directly.

## Performance

- File notifications wake a background cache worker.
- Only changed files are read, parsed, and validated again.
- A five-second metadata check recovers missed notifications. It does not
  reparse unchanged documents or redraw an idle screen.
- Mouse input and rendering use memory snapshots. They do not read flow files.
- The call tree is rebuilt only after a data, flow, or fold change.
- The terminal writes only changed screen rows. There is no redraw timer.
- Invalid updates keep the last valid snapshot and show an error.
- Herdr commands and flow deletion run outside the input loop.

## Format

The format follows the documented fields of
[bb-plugin-callstack](https://github.com/ebg1223/bb-plugin-callstack).
This is an independent implementation. It does not use BB runtime code.

Flow fields: `name`, `description`, `status` (`current` or `proposed`), `frames`,
and `types` (a map from type names to definitions).

Frame fields: `fn`, `loc`, `in`, `out`, `cond`, `loop`, `change`, `concurrent`,
`module`, `note`, `detail`, and nested `calls`. `fn` is required. `change` is
`same`, `added`, `modified`, or `removed`.

Limits: 2 MB input, 50 calls per group, 20 nested levels, and 5000 total calls.
Control characters in supplied text are replaced before terminal display.
The plugin detects source-file changes. It does not generate a new flow itself.

## Development checks

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests cover record compatibility, input limits, concurrent publishing, cache
reuse, file notifications, mouse controls, source paths, and command timeouts.

To measure cached rendering with your own flow data:

```sh
cargo run --release --locked --example benchmark -- /absolute/state/directory NAMESPACE WORKSPACE SESSION
```

This measures screen construction. It excludes terminal drawing and initial loading.
