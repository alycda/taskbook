# CLI Reference

## Synopsis

```
tb [--taskbook-dir PATH] [COMMAND] [ARGS...]
```

Running `tb` with no command opens the interactive board (TUI). Every other
action is a subcommand: `tb task ...`, `tb check 1`, and so on. Run
`tb <command> --help` for the arguments of one command.

Two rules hold across every command:

- **Item ids are bare numbers** (`3`). Commands that act on items accept one
  or more leading ids: `tb check 1 2`, `tb priority 3 4 p1`.
- **`@name` always names a board.** `@3` is never an id; taskbook 1.x used
  that form for some commands, and 2.0 rejects it with a hint.

Upgrading from 1.x? See [Upgrading to 2.0](upgrading-to-2.0.md).

## Display Commands

### Board View

```bash
tb board
```

Prints all items grouped by board, followed by overall progress. Use this
instead of `tb` when you want non-interactive output (scripts, pipes).

### Timeline View

```bash
tb timeline
```

Prints items grouped by creation date.

### Archive View

```bash
tb archive
```

Prints archived (deleted) items. The ids shown here are archive ids, used by
`tb restore`.

### Show Items

```bash
tb show <ID>... [--archived]
```

Prints every field of the given items: state, priority, due date, boards,
tags, star, creation date, and the full body of notes. `--archived` looks up
archive ids (as shown by `tb archive`).

```bash
tb show 3
tb show 3 4
tb show --archived 1
```

In the TUI, `/show <id>...` narrows the current view to those items; Esc
clears it.

## Creating Items

### Create Task

```bash
tb task [@BOARD]... [+TAG]... [p:1-3] [due:DATE] <DESCRIPTION>...
```

Tokens may appear in any order; everything that is not a board, tag,
priority or due date is the description.

**Boards**: prefix board names with `@`. Without one, the task goes to
`My Board`.

```bash
tb task @work Review pull request
tb task @coding @review Implement feature X     # Multiple boards
```

**Priority**: `p:1` = high, `p:2` = medium, `p:3` = normal (the default).
`p:high`, `p:medium` and `p:normal` also work. Any other `p:` value is an
error.

```bash
tb task Fix critical bug p:1          # High priority
tb task @work Update docs p:2         # Medium priority
```

**Tags**: prefix tags with `+`.

```bash
tb task @coding +urgent +frontend Fix login bug
```

**Due date**: `due:` followed by `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`, `today`,
`tomorrow`, `now`, or `today+HHMM` / `tomorrow+HHMM` / `now+HHMM`. A token
cannot contain a space, so use the `T` separator for a time.

```bash
tb task Pay rent due:2026-07-01
tb task Call dentist due:tomorrow+0930
```

### Create Note

```bash
tb note [@BOARD]... [+TAG]... [TEXT]...
tb note
```

Creates a note. Notes cannot be checked off and have no priority or due date
(`p:` and `due:` tokens are rejected).

With no text, opens your external editor (`$EDITOR` or `$VISUAL`, falling
back to `vi`) to compose a title and optional body. Lines starting with `//`
are comments. Delete all content to cancel.

```bash
tb note API endpoint: https://api.example.com
tb note @meetings Standup at 9am daily
tb note                      # Opens editor for a multi-line note
```

## Modifying Items

Every command below validates all of its ids before changing anything: if
one id does not exist (or a task-only command is given a note), nothing is
changed and every offending id is reported.

### Check / Uncheck Task

```bash
tb check <ID>...
```

Toggles tasks between done and pending.

### Begin / Pause Task

```bash
tb begin <ID>...
```

Toggles tasks between in-progress and pending.

### Star / Unstar Item

```bash
tb star <ID>...
```

Toggles the star on tasks and notes.

### Edit Description

```bash
tb edit <ID> <DESCRIPTION>...
```

Replaces one item's description. Everything after the id is the new text,
including words that start with `@` or `+`.

```bash
tb edit 3 Merge PR #42
```

### Edit Note in Editor

```bash
tb edit-note <ID>
```

Opens a note in your external editor to edit its title and body. The first
non-comment line becomes the title; the remaining lines become the body.

### Set Priority

```bash
tb priority <ID>... <LEVEL>
```

The last argument is the level: `1`/`high`, `2`/`medium`, `3`/`normal`
(`p1` and `p:1` also work). Tasks only.

```bash
tb priority 5 1          # High
tb priority 5 6 medium   # Medium, two tasks
```

### Set or Clear Due Date

```bash
tb due <ID>... <DATE|none>
```

Accepts the `due:` formats above, plus `"YYYY-MM-DD HH:MM"` (quoted or not).
`none` clears the due date. Tasks only.

```bash
tb due 3 2026-07-01
tb due 3 2026-07-01 14:30
tb due 3 4 tomorrow
tb due 3 none
```

### Move to Boards

```bash
tb move <ID>... <@BOARD>...
```

Puts items on exactly the given boards, replacing their current boards. The
`@` is optional here, but a board whose name is a number must be written
with it (`@2026`), since bare numbers are ids.

```bash
tb move 3 @work
tb move 3 4 @work @review
```

### Add / Remove Tags

```bash
tb tag <ID>... <+TAG|-TAG>...
```

```bash
tb tag 3 +urgent +frontend
tb tag 3 4 -urgent
```

## Deleting and Restoring

### Delete Items

```bash
tb delete <ID>...
```

Moves items to the archive. Archived items get new (archive) ids.

### Restore from Archive

```bash
tb restore <ID>...
```

Moves archived items back, using the ids shown by `tb archive`. Restored
items get new ids.

### Clear Completed

```bash
tb clear
```

Moves every completed task to the archive.

## Searching and Filtering

### Find Items

```bash
tb find <TERM>...
```

Shows items where any term matches the description, note body or tags
(case-insensitive substring). A `+term` only matches tags.

```bash
tb find documentation
tb find "pull request"
tb find +urgent
```

### List by Filters

```bash
tb list [FILTER]...
```

Shows items matching every filter. With no filters, same as `tb board`.

| Filter | Matches |
|--------|---------|
| `pending` | Tasks neither done nor in progress |
| `progress` | In-progress tasks (also `started`, `begun`, `in_progress`) |
| `done` | Completed tasks (also `checked`, `complete`) |
| `task` | All tasks (also `tasks`, `todo`) |
| `note` | All notes (also `notes`) |
| `starred` | Starred items (also `star`) |
| `+tag` | Items with that tag |
| `@board` | Restricts the view to that board (must exist) |

A bare word that is not a filter is accepted if it names an existing board;
anything else is an error (so a typo like `pendng` fails instead of showing
everything).

```bash
tb list pending
tb list done starred
tb list pending @coding +urgent
```

## Clipboard

```bash
tb copy <ID>...
```

Copies item descriptions to the system clipboard, one per line.

## Server Commands

These commands sync with a remote server. See [Sync & Encryption](sync.md)
for details. Each prompts interactively for values not given as options.

### Register Account

```bash
tb register [--server URL] [--username NAME] [--email EMAIL] [--password PASS]
```

```bash
tb register                      # Fully interactive (recommended)
tb register --server https://taskbook.example.com --username alice --email alice@example.com
```

### Login

```bash
tb login [--server URL] [--username NAME] [--password PASS] [--key BASE64]
```

`--key` is the encryption key printed at registration.

### Logout

```bash
tb logout
```

Logs out and deletes stored credentials. The client returns to local-only
mode.

### Status

```bash
tb status
```

Shows sync status, server URL, and whether credentials are saved.

### Migrate Local Data

```bash
tb migrate
```

Pushes existing local data to the server. Use this after registering to
upload your existing items.

### MCP Server

```bash
tb mcp
```

Runs a Model Context Protocol server over stdio (newline-delimited JSON-RPC
2.0) against the configured storage backend. Register it with Claude Code:

```bash
claude mcp add taskbook -- tb mcp
```

Tools: `list_items`, `list_boards`, `create_task`, `create_note`,
`set_task_state`, `edit_item`, `delete_items`, `restore_items`. The
`priority` field uses the same scale as the CLI (1 = high … 3 = normal).

## Global Options

### Custom Taskbook Directory

```bash
tb --taskbook-dir <PATH> [COMMAND]
```

Uses a custom directory instead of `~/.taskbook/`. Can also be set with the
`TASKBOOK_DIR` environment variable.

```bash
tb --taskbook-dir ~/work-tasks task Work item
TASKBOOK_DIR=~/work-tasks tb board
```

### Help and Version

```bash
tb --help             # All commands
tb <command> --help   # One command
tb --version          # Also -v
```

## Exit Status and Errors

`tb` exits `0` on success, `1` when an operation fails (unknown id, note
given a priority, invalid date, ...), and `2` for a malformed command line
(unknown command, non-numeric id, missing argument). Each error is printed
once, to stderr.

## Examples

```bash
# Daily workflow
tb                                    # Interactive board
tb task @work Code review p:1         # Create a high-priority task
tb begin 1                            # Start working
tb check 1                            # Mark done

# Organize
tb task @personal Buy groceries p:2 due:tomorrow
tb star 3
tb move 3 @shopping
tb tag 3 +errands

# Search and filter
tb find meeting
tb list pending starred
tb timeline

# Cleanup
tb delete 5 6 7
tb clear
tb archive                            # View deleted items
tb restore 5                          # Restore by archive id

# Server sync
tb register
tb migrate
tb status
```
