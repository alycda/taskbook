# CLI Reference

## Synopsis

```
tb [OPTIONS] [INPUT...]
```

## Item IDs

Each item has a numeric ID, shown next to it on the board. IDs are not
stored in a counter: a new ID is always `highest existing ID + 1`, computed at
the moment it is needed (the same rule as the original Node.js taskbook, so
the storage format stays compatible). Consequences:

- **Only the top ID is reused.** Deleting the highest-numbered item frees its
  number for the next item you create. Deleting an item in the middle leaves
  a gap that is never filled.
- **The board and the archive count separately.** Deleting an item moves it
  to the archive under a new *archive* ID (`highest archive ID + 1`).
  `tb --archive` shows those archive IDs, and `--restore` takes them.
- **Restoring assigns a new board ID.** A restored item gets
  `highest board ID + 1`, not the ID it had before it was deleted.

```bash
tb --delete 3       # task 3 becomes, e.g., archive item 7
tb --archive        # look up its archive ID
tb --restore 7      # comes back with the next free board ID, not 3
```

## Display Commands

### Board View (default)

```bash
tb
```

Displays all items organized by board. This is the default when no options are provided.

### Timeline View

```bash
tb --timeline
tb -i
```

Displays items in chronological order by creation date.

### Archive View

```bash
tb --archive
tb -a
```

Displays archived (deleted) items.

## Creating Items

### Create Task

```bash
tb --task <description>
tb -t <description>
```

Creates a new task. Tasks can be checked off when completed.

**Board assignment**: Prefix board names with `@`:

```bash
tb --task @work "Review pull request"
tb --task @coding @review "Implement feature X"  # Multiple boards
```

**Priority**: Append `p:N` where N is 1 (normal), 2 (medium), or 3 (high):

```bash
tb --task "Fix critical bug" p:3        # High priority
tb --task @work "Update docs" p:2       # Medium priority
```

**Tags**: Add `+tag` words (see [Tags](#tags)):

```bash
tb --task "Fix login bug" +backend +urgent
```

**Combined example**:

```bash
tb --task @work @urgent "Deploy hotfix" p:3 +ops
```

Board names, priority, tags and `due:` are recognized only when they are
separate arguments. A quoted argument is kept as literal description text:
`tb --task "Fix +urgent"` creates a task whose description is `Fix +urgent`
and which has no tags.

### Create Note

```bash
tb --note <description>
tb -n <description>
tb --note
```

Creates a note. Notes are for reference and cannot be checked off.

When called without a description, opens your external editor (`$EDITOR` or `$VISUAL`, falling back to `vi`) to compose a note with a title and optional body. Lines starting with `//` are treated as comments and ignored. Delete all content to cancel.

```bash
tb --note "API endpoint: https://api.example.com"
tb --note @meetings "Standup at 9am daily"
tb --note                   # Opens editor for multi-line note
```

## Modifying Items

### Check/Uncheck Task

```bash
tb --check <id> [id...]
tb -c <id> [id...]
```

Toggles the completion status of tasks. Multiple IDs can be specified.

```bash
tb --check 1
tb --check 1 2 3
```

### Begin/Pause Task

```bash
tb --begin <id> [id...]
tb -b <id> [id...]
```

Toggles the in-progress status of tasks. In-progress tasks are highlighted differently in the board view.

```bash
tb --begin 2
tb --begin 4 5
```

### Star/Unstar Item

```bash
tb --star <id> [id...]
tb -s <id> [id...]
```

Toggles the starred status. Works on both tasks and notes.

```bash
tb --star 1
tb --star 1 3 5
```

### Edit Description

```bash
tb --edit @<id> <new description>
tb -e @<id> <new description>
```

Replaces an item's description. The ID must be prefixed with `@`.

`+tag` arguments are **added** to the item's existing tags rather than becoming
part of the description; existing tags are never removed by `--edit` (use
[`--tag`](#tag-item) for that). Quote the text to keep a `+word` literally.

```bash
tb --edit @3 "Updated task description"
tb --edit @3 Updated task description +review   # also adds tag "review"
tb --edit @3 "Give this a +1"                   # literal "+1", no tag
```

### Edit Note in Editor

```bash
tb --edit-note @<id>
```

Opens a note in your external editor (`$EDITOR` or `$VISUAL`, falling back to `vi`) for editing both the title and body. The first non-comment line becomes the title, and remaining lines become the body. Lines starting with `//` are comments and ignored. Delete all content to cancel.

```bash
tb --edit-note @5
```

### Set Priority

```bash
tb --priority @<id> <1-3>
tb -p @<id> <1-3>
```

Sets task priority: 1 = normal, 2 = medium, 3 = high.

```bash
tb --priority @5 3    # Set to high priority
tb --priority @5 1    # Set to normal priority
```

### Tag Item

```bash
tb --tag @<id> [+tag...] [-tag...]
```

Adds (`+tag`) and removes (`-tag`) tags on one item. Removals are applied
before additions. Removing a tag the item does not have prints a warning and
changes nothing; the command still succeeds.

```bash
tb --tag @3 +urgent +frontend
tb --tag @3 -urgent
```

### Move to Board

```bash
tb --move @<id> <board>
tb -m @<id> <board>
```

Moves an item to a different board. Opens a board picker if the board name is omitted.

```bash
tb --move @3 work
tb --move @3 @personal   # @ prefix is optional for board name
```

## Deleting and Restoring

### Delete Item

```bash
tb --delete <id> [id...]
tb -d <id> [id...]
```

Moves items to the archive. They can be restored later.

```bash
tb --delete 4
tb --delete 1 2 3
```

### Restore from Archive

```bash
tb --restore <id> [id...]
tb -r <id> [id...]
```

Restores archived items back to the active board. The IDs are **archive**
IDs as shown by `tb --archive`, not the IDs the items had before deletion.
Each restored item gets a new board ID (see [Item IDs](#item-ids)).

```bash
tb --restore 4
tb --restore 10 11 12
```

### Clear Completed

```bash
tb --clear
```

Permanently deletes all completed (checked) tasks from all boards.

## Tags

Tags are an addition in this Rust port; the original Node.js taskbook has
none. An item can have any number of tags, shown after its description as
`+tag`.

- A tag is written as an argument starting with `+` and at least one more
  character (`+urgent`). A lone `+` is ordinary text.
- Tags are stored lowercase without the `+`, so `+Urgent` and `+urgent` are
  the same tag. Duplicates are dropped.
- Any characters are allowed after the `+` (`+a,b` is the single tag `a,b`).
- Tags can be set on create (`--task`, `--note`), added by `--edit`, and added
  or removed by `--tag`. They are kept when an item is deleted and restored.
- Filtering: `--find +x` matches tags *containing* `x`; `--list +x` matches
  the tag `x` exactly.

In the TUI, the same `+tag` syntax works in `/task`, `/note` and `/edit`, and
`/tag @<id> +add -remove` edits tags. Because the TUI splits on whitespace,
every `+word` there becomes a tag. MCP clients pass tags as arrays
(`create_task.tags`, `edit_item.tags` replaces the whole list, `list_items.tag`
filters by one exact tag).

## Searching and Filtering

### Find Items

```bash
tb --find <search term>
tb -f <search term>
```

Searches for items whose description, note body, or tags contain the search
term (case-insensitive substring). With several terms, an item matches if
**any** term matches. A term starting with `+` searches tags only, still as a
substring.

```bash
tb --find documentation
tb --find "pull request"
tb --find +front        # matches tags "frontend", "front", ...
```

### List by Attributes

```bash
tb --list <attributes>
tb -l <attributes>
```

Filters items by one or more attributes:

| Attribute | Description |
|-----------|-------------|
| `pending` | Incomplete tasks |
| `done` | Completed tasks |
| `task` | All tasks |
| `note` | All notes |
| `starred` | Starred items |

`+tag` filters by tag. Unlike `--find`, the tag must match exactly
(case-insensitive), and with several `+tag` filters an item must have **all**
of them.

```bash
tb --list pending
tb --list done starred
tb --list task pending    # Pending tasks only
tb --list +urgent +backend pending
```

## Clipboard

### Copy to Clipboard

```bash
tb --copy <id> [id...]
tb -y <id> [id...]
```

Copies item descriptions to the system clipboard. Multiple items are joined with newlines.

```bash
tb --copy 1
tb --copy 1 2 3
```

## Server Commands

These commands are used for syncing with a remote server. See [Sync & Encryption](sync.md) for details.

### Register Account

```bash
tb --register [OPTIONS]
```

Creates a new account on the sync server. Prompts interactively for missing values.

| Option | Description |
|--------|-------------|
| `--server <url>` | Server URL |
| `--username <name>` | Username |
| `--email <email>` | Email address |
| `--password <pass>` | Password (prompted securely if omitted) |

```bash
# Fully interactive (recommended)
tb --register

# Partial - prompts for password
tb --register --server https://taskbook.example.com --username alice --email alice@example.com
```

### Login

```bash
tb --login [OPTIONS]
```

Logs in to an existing account. Prompts interactively for missing values.

| Option | Description |
|--------|-------------|
| `--server <url>` | Server URL |
| `--username <name>` | Username |
| `--password <pass>` | Password (prompted securely if omitted) |
| `--key <base64>` | Encryption key from registration |

```bash
# Fully interactive
tb --login

# With server specified
tb --login --server https://taskbook.example.com
```

### Logout

```bash
tb --logout
```

Logs out and deletes stored credentials. Sync is disabled and the client returns to local-only mode.

### Status

```bash
tb --status
```

Shows current sync status, server URL, and whether credentials are saved.

### Migrate Local Data

```bash
tb --migrate
```

Pushes existing local data to the server. Use this after registering to upload your existing tasks.

## Global Options

### Custom Taskbook Directory

```bash
tb --taskbook-dir <path>
```

Use a custom directory for storing taskbook data instead of `~/.taskbook/`.

```bash
tb --taskbook-dir ~/work-tasks --task "Work item"
```

Can also be set via environment variable:

```bash
export TASKBOOK_DIR=~/work-tasks
tb --task "Work item"
```

### CLI Mode

```bash
tb --cli
```

Runs in non-interactive CLI mode, printing output to stdout instead of launching the interactive TUI. Useful for scripting or piping output.

### Help

```bash
tb --help
tb -h
```

Displays help message with all available options.

### Version

```bash
tb --version
tb -v
```

Displays the installed version.

## Examples

```bash
# Daily workflow
tb                                    # View boards
tb --task @work "Code review"         # Create task
tb --begin 1                          # Start working
tb --check 1                          # Mark done

# Organize tasks
tb --task @personal "Buy groceries" p:2
tb --star 3
tb --move @3 shopping

# Search and filter
tb --find meeting
tb --list pending starred
tb --timeline

# Cleanup
tb --delete 5 6 7
tb --clear
tb --archive                          # View deleted items and their archive IDs
tb --restore 2                        # Oops, restore one (by archive ID)

# Server sync
tb --register
tb --migrate
tb --status
```
