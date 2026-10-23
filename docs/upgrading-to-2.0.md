# Upgrading to 2.0

Taskbook 2.0 drops several behaviors inherited from the original Node.js
taskbook. Everything below is a breaking change to the command line, the TUI
command line, or the MCP tools. Your data does not need a manual migration.

## Commands are subcommands

Actions are subcommands instead of flags.

| 1.x | 2.0 |
|-----|-----|
| `tb --task @work Fix it` | `tb task @work Fix it` |
| `tb --check 1 2` | `tb check 1 2` |
| `tb --cli` | `tb board` |
| `tb --register --server URL` | `tb register --server URL` |
| `tb --mcp` | `tb mcp` |

The 1.x flags (`--task`/`-t`, `--check`/`-c`, ...) still work in 2.x: they
are rewritten to the matching subcommand and print a deprecation warning on
stderr. They will be removed in 3.0. Update scripts and your MCP
registration:

```bash
claude mcp remove taskbook
claude mcp add taskbook -- tb mcp
```

`tb <words>` with no command used to print the board and ignore the words;
it is now an "unrecognized subcommand" error.

## Ids never take `@`

In 1.x, `@` meant an item id for `--edit`, `--move`, `--priority`, `--due`,
`--tag` and `--edit-note`, but a board everywhere else. In 2.0 ids are always
bare numbers and `@` always names a board.

| 1.x | 2.0 |
|-----|-----|
| `tb --edit @3 New text` | `tb edit 3 New text` |
| `tb --move @3 work` | `tb move 3 @work` |
| `tb --priority @3 3` | `tb priority 3 1` (see below) |
| `tb --due @3 tomorrow` | `tb due 3 tomorrow` |
| `tb --tag @3 +x` | `tb tag 3 +x` |

`tb edit @3 ...` is rejected with a hint (`use 3`), so old muscle memory
fails loudly instead of doing something else. The same applies to the TUI
command line (`/edit 3 ...`, `/move 3 @work`, `/priority 3 1`).

`move`, `priority`, `due` and `tag` now accept several ids:
`tb priority 3 4 5 high`.

## Priority 1 is the highest

| Meaning | 1.x | 2.0 |
|---------|-----|-----|
| High (`!!`) | 3 | **1** |
| Medium (`!`) | 2 | 2 |
| Normal (default) | 1 | **3** |

This applies to `p:N` when creating a task, to `tb priority`, to the TUI,
and to the MCP `priority` field (input and output). Labels work too:
`p:high`, `tb priority 3 medium`.

**Stored data.** 2.0 writes the new scale to a new `_priority` field and
also keeps writing the old `priority` field with the 1.x value, so:

- Existing 1.x data (and data from the Node.js taskbook) is read correctly;
  nothing to migrate.
- A 1.x client sharing the same directory or sync server keeps reading
  correct priorities from items 2.0 wrote. When 1.x saves an item it drops
  `_priority`, and 2.0 falls back to the legacy field.

## Stricter input

Input that 1.x silently ignored is now an error, and nothing is changed:

- A non-numeric id: `tb check 1 foo` used to check 1 and ignore `foo`.
- A missing id: the whole command fails and lists every missing id, rather
  than stopping at the first one.
- A task-only command on a note: `check` and `begin` on a note used to do
  nothing, and `priority` reported success without changing anything.
- An invalid `p:` token: `tb task Fix p:4` used to create a task described as
  "Fix p:4".
- `p:` or `due:` on a note: previously dropped without a word.
- An unknown `tb list` filter: `tb list pendng` used to show everything. An
  `@board` that does not exist is also an error.
- A `tb tag` argument that is neither `+tag` nor `-tag`.
- MCP: an `ids` array containing a non-integer, and an `edit_item` call that
  mixes valid and invalid fields. `edit_item` now validates every field
  before applying any of them, and applies them in one write.

## Error output

Each error is printed once, to stderr (1.x printed a specific message and
then a generic `Invalid item ID: 0`). Exit status is `1` for a failed
operation and `2` for a malformed command line.

In the TUI, a failing slash command (`/delete 99`, `/edit 99 x`) shows the
error in the status bar. Previously it closed the TUI.
