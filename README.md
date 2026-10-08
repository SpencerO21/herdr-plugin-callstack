# Call stacks for Herdr

A Rust and Ratatui plugin that shows agent-supplied call flows in a terminal pane.
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
Publishing remains an agent or command-line operation. **Tools > Generate** and
**Tools > Update selected flow** can request publication from an existing idle
Herdr agent. They do not start a new agent automatically.

## Mouse and keyboard

| Mouse control | Keyboard | Result |
| --- | --- | --- |
| Click a function | Up/Down or j/k | Select a call |
| Click `[+]` or `[-]`, or Fold | Enter or Space | Expand or fold nested calls |
| Double-click a function, or Open nvim | o | Open the source line in a new Neovim pane |
| Diff dn | g | Open the selected file's changes in DiffNav through `dn` |
| Prev / Next | Shift+Tab / Tab | Change flows |
| Details / Hide details | d | Show types, conditions, and notes below the stack |
| More / Back | m | Show or hide extra controls |
| Tree / Diagram / Combined | 1 / 2 / 3 | Select the view |
| Tools | t | Open search, filters, preview, history, and agent controls |
| Tools > Search functions | / | Find a function and jump to it |
| Tools > Focus selected path | f | Hide unrelated branches; repeat to clear |
| Tools > Changes only | x | Keep changed calls and their ancestors |
| Tools > Source preview | s | Read source below the tree or diagram |
| Tools > Module lanes | 4 | Group active flows by module |
| Tools > History | h | Switch between active and archived flows |
| Tools > Update selected flow | r | Prepare an agent request; confirmation is required |
| Wheel, Up/Down, Page up/down | Arrows, PageUp/PageDown | Scroll |
| Left / Right | Left/Right | Read long tree rows |
| Expand all / Fold all | — | Change the whole tree |
| Delete, then Yes, delete | — | Remove the selected flow |
| Cancel | Escape | Cancel deletion |
| Quit | q or Ctrl+C | Close the view |

The legend stays visible above the status line:
`=` same, `+` added, `~` modified, and `-` removed.
The boxed `[+]` and `[-]` controls fold calls; they are not change labels.

The stack stays visible when you open Details. Click a call to read its details.
The selected call has a highlighted row. Its source path appears below the tree.
Source warnings have a separate status line. Extra controls, including Delete,
are under **More**. The mouse wheel scrolls details while Details is open.
Set `NO_COLOR=1` to disable colours. `[parallel]` marks calls that can start together.

Buttons wrap in narrow panes. The full view needs at least 29 columns and 16 rows.
Smaller panes show a resize message and a mouse-accessible Quit button.
Terminal mouse handling and the normal screen are restored on exit.

## Search, filters, and previews

Open **Tools** in either the tree or diagram. Every tool has a mouse control.
Search and agent requests accept typed text. The search field owns keyboard
input while open, so typing shortcut letters does not trigger viewer actions.

- Search matches function names without case sensitivity. It searches the
  active flows, or archived flows while History is open. It shows up to 500
  matches. Click a result to unfold and center it. A search jump clears path
  and change filters so the result is visible.
- Path focus keeps the selected function, its callers, and its descendants.
  It removes unrelated sibling branches. Shared functions can retain callers
  from multiple flows. The filter does not change saved data.
- Changes only keeps calls marked added, modified, or removed, plus their
  ancestors. It combines with path focus. Use **Clear all filters** to reset.
- Trace a type lets you choose an input/output type name. Matching calls remain
  bright; other calls become dim. A star also identifies matches without color.
  Matching uses identifier tokens, not substring matching or type inference.
- Source preview is a read-only snapshot below the tree or diagram. It shows
  line numbers, up to 20 preceding lines, and 60 following lines. Scroll the
  detail area to read it. Click Preview again to reload. Click Details to return
  to call metadata. Preview reads run outside the input loop.

Preview rejects files outside the project, non-UTF-8 or binary files, files
over 4 MB, and invalid source lines. Selecting another call closes the old
preview. Late preview results cannot replace a different call's content.

## Module lanes

**Tools > Module lanes** groups functions into labeled columns. It combines
the currently visible flows. The frame's module field sets the column;
missing modules use **Other**. A shared function uses its first occurrence's
module. Version details still show each occurrence's module.

Use the normal diagram movement controls to reach other columns. Compact,
Expanded, focus, changes-only, type tracing, and source controls also work here.

## Archive and restore

**Tools > Archive selected flow** hides the selected flow from active views.
**Tools > History** shows archived flows. Select a flow there and use
**Restore selected flow** to return it to active views. Archiving is reversible.
Deletion is still separate and requires confirmation.

CLI equivalents are archive NAME and restore NAME. The list command includes
archived flows and labels them. The view --once command prints active flows only.

Archive state uses a separate local marker file. It does not rewrite flow data
or source hashes. Publishing the same name preserves its archive state.
History is an archive list, not a revision log. Publishing still replaces the
previous data for that flow name.

## Generate and update flows

1. Open **Tools > Generate a new flow**, or **Update selected flow** (r).
2. Enter the function or code path to inspect.
3. Click **Choose agent**. Only idle/done agents in this workspace and project
   folder are eligible. Agents in subfolders of the project are also eligible.
4. Choose an agent. Review the target, project, and request.
5. Click **Send request**. No request is sent before this confirmation.

The plugin checks readiness and agent session identity again before submission.
It never stops an agent or answers an agent approval prompt. If no eligible
agent exists, start one in the project and try again. A Herdr server that does
not support agent prompts reports an error; the plugin does not send raw keys
as a fallback.

The request asks the agent to read code and publish to this exact namespace,
workspace, session, and state folder. It forbids source edits, commits, pushes,
flow deletion, and external services. Update includes the current flow data
and asks the agent to keep its name. The agent's normal permissions still apply.
Requests may use agent credits.

“Request sent” confirms submission only. It does not mean the flow is ready.
Published data arrives through the existing file watcher. The plugin does not
guarantee an agent's response or automatically retry failed submissions.

## Flow diagrams

Click **Diagram** to show the selected flow as function boxes and arrows.
Click **Combined** to show all saved flows in this session in one diagram.
Click **Tree** to return to the nested call list. The diagrams need at least
29 columns and 24 rows. A smaller pane keeps a Tree button available.

- **Expanded** shows source locations, input/output types, conditions, and loops.
  **Compact** uses smaller boxes. Press `z` to switch sizes.
- Click a box to select it and show details below the diagram.
- Double-click a box, or click **Open nvim**, to open its source.
- Click **Diff dn** to open the selected version's file changes in DiffNav.
- Use the mouse wheel to move vertically. Shift+wheel moves horizontally
  when the terminal supplies the Shift modifier. Horizontal wheel events also work.
- **More** has buttons for all movement directions, page movement, and **Center**.
  Arrow keys move the diagram. `c` centers the selected box.
- **Prev call** and **Next call** select and center boxes. Their keys are `p` and `n`.
- **Version** cycles through a shared function's occurrences. Its key is `v`.
  The details show the flow name, version number, and source location.
- Move the wheel over the details to scroll them. **More** also has **Details up**
  and **Details down** buttons. `d` shows or hides details.

An arrow means that the parent calls the child. Dashed connectors mark conditions.
Double-line connectors belong to multiple flows. A side connector marked `return`
shows a backward or recursive link. Loop and parallel-call labels appear in boxes.
The `╳` symbol marks crossing lines, not a connection. Select a function to
highlight its incoming and outgoing connectors.
The usual `=`, `+`, `~`, and `-` change markers remain visible without color.
Mixed markers mean that flow versions have different change labels.

The combined view joins functions by project folder, file path, and function name.
Line numbers can differ. Same-name functions in different files stay separate.
Functions without a source location stay within their own flow. This is a display
rule, not symbol analysis: same-name functions within one file can still join.
Use **Version** to inspect each occurrence. Source opening and deletion use that
occurrence's flow. Delete still requires confirmation.

The diagrams show the published call data. They do not run code or start an agent.
Graph layout stays in memory. Drawing allocates only the visible diagram area.
Changing sizes alters the box layout; it does not shrink the terminal font.

To try the bundled sample, run these commands from this repository in Herdr:

```sh
./target/release/herdr-callstack publish examples/diagram-checkout.json --session diagram-demo
./target/release/herdr-callstack publish examples/diagram-retry.json --session diagram-demo
./target/release/herdr-callstack open --session diagram-demo
```

Click **Combined**, then **Expanded**. The sample code makes no real charges.

For a larger example with 3 flows and 21 shared or distinct functions:

```sh
./target/release/herdr-callstack publish examples/commerce-checkout.json --session commerce-demo
./target/release/herdr-callstack publish examples/commerce-retry.json --session commerce-demo
./target/release/herdr-callstack publish examples/commerce-refund.json --session commerce-demo
./target/release/herdr-callstack open --session commerce-demo
```

Use **Combined** and **Compact** for the overview. Scroll down to see the
notification branches. Select `process_payment` or `publish_event`, then use
**Version** to compare flow contexts. The example includes parallel checks,
conditional calls, bounded retries, and proposed notification changes.
All source functions are local stubs. They make no real payments or requests.

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
- Ratatui writes only changed terminal cells. There is no redraw timer.
- Graph structure is rebuilt only when data or the view changes. Moving the
  diagram reuses its layout. Changing size recalculates box positions only.
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
Diagram tests cover Ratatui output, shared functions, cycles, source actions,
version selection, narrow panes, Unicode text, and viewport-sized allocation.

To measure cached rendering with your own flow data:

```sh
cargo run --release --locked --example benchmark -- /absolute/state/directory NAMESPACE WORKSPACE SESSION
```

This measures screen construction. It excludes terminal drawing and initial loading.
