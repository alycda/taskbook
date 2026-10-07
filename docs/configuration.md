# Configuration

Client configuration is stored in `~/.config/taskbook/taskbook.json` (honoring `$XDG_CONFIG_HOME`). The file is created automatically with default values on first run.

For backward compatibility, if a legacy `~/.taskbook.json` exists and the XDG file does not, the legacy file is used. To migrate, move `~/.taskbook.json` to `~/.config/taskbook/taskbook.json`.

## Configuration File

```json
{
  "taskbookDirectory": "~",
  "displayCompleteTasks": true,
  "displayProgressOverview": true,
  "theme": "default",
  "sortMethod": "id",
  "sync": {
    "enabled": false,
    "serverUrl": "http://localhost:8080"
  }
}
```

## Options

### taskbookDirectory

**Type**: `string`
**Default**: `"~"`

Directory where taskbook stores its data. The actual storage location is `<taskbookDirectory>/.taskbook/`.

```json
{
  "taskbookDirectory": "~"
}
```

Results in data stored at `~/.taskbook/storage/storage.json`.

You can also specify this directory:
- Via CLI: `tb --taskbook-dir /path/to/dir`
- Via environment variable: `TASKBOOK_DIR=/path/to/dir`

Priority order (highest to lowest):
1. `--taskbook-dir` CLI flag
2. `TASKBOOK_DIR` environment variable
3. `taskbookDirectory` in config file
4. Default (`~`)

### displayCompleteTasks

**Type**: `boolean`
**Default**: `true`

Whether to show completed tasks in the board view.

```json
{
  "displayCompleteTasks": false
}
```

When `false`, completed tasks are hidden from the board view but still exist and can be seen with `tb --list done`.

### displayProgressOverview

**Type**: `boolean`
**Default**: `true`

Whether to show the progress statistics at the bottom of the board view.

```json
{
  "displayProgressOverview": false
}
```

### theme

**Type**: `string` or `object`
**Default**: `"default"`

Color theme for the terminal output. Can be a preset name or custom color definitions.

#### Preset Themes

```json
{
  "theme": "catppuccin-macchiato"
}
```

Available presets:

| Theme | Description |
|-------|-------------|
| `default` | Neutral colors, works on most terminals |
| `catppuccin-macchiato` | [Catppuccin](https://catppuccin.com/) Macchiato variant |
| `catppuccin-mocha` | Catppuccin Mocha (darkest) |
| `catppuccin-frappe` | Catppuccin Frappé |
| `catppuccin-latte` | Catppuccin Latte (light theme) |
| `high-contrast` | High contrast for accessibility |

#### Custom Theme

Define custom RGB colors for each element:

```json
{
  "theme": {
    "muted": { "r": 140, "g": 140, "b": 140 },
    "success": { "r": 134, "g": 239, "b": 172 },
    "warning": { "r": 253, "g": 224, "b": 71 },
    "error": { "r": 252, "g": 129, "b": 129 },
    "info": { "r": 147, "g": 197, "b": 253 },
    "pending": { "r": 216, "g": 180, "b": 254 },
    "starred": { "r": 253, "g": 224, "b": 71 }
  }
}
```

| Color | Used For |
|-------|----------|
| `muted` | Secondary text (IDs, labels, completed task text) |
| `success` | Checkmarks, completed counts, normal priority |
| `warning` | In-progress indicators, medium priority |
| `error` | Error messages, high priority |
| `info` | Notes, information text |
| `pending` | Pending task icons and counts |
| `starred` | Star indicators |

### sortMethod

**Type**: `string`
**Default**: `"id"`

Sort method for items within boards. Can be cycled in the TUI with a keybinding.

| Value | Description |
|-------|-------------|
| `id` | Sort by item ID (creation order) |
| `priority` | Sort by priority (high first), then ID |
| `status` | Sort by status (pending, in-progress, done), then ID |

```json
{
  "sortMethod": "priority"
}
```

### sync

**Type**: `object`
**Default**: `{ "enabled": false, "serverUrl": "http://localhost:8080" }`

Configuration for server sync. This is typically managed automatically by the `--register` and `--login` commands.

```json
{
  "sync": {
    "enabled": true,
    "serverUrl": "https://taskbook.example.com"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `enabled` | `boolean` | Whether sync is active |
| `serverUrl` | `string` | URL of the sync server (`backend = "server"`) |
| `backend` | `"server"` \| `"ditto"` | Which sync backend to use; defaults to `"server"` |

When `enabled` is `true`, all task operations go to the selected backend instead of local files.

See [Sync & Encryption](sync.md) for the server backend and [Ditto Backend](ditto.md) for the peer-to-peer backend.

### ditto

**Type**: `object`
**Default**: `{ "connect": "peers", "provider": "development", "collection": "taskbook_items", "encrypt": true, "flushTimeoutMs": 1000 }`

Settings for the Ditto backend (`sync.backend = "ditto"`, requires a build with `--features ditto`). Secrets live in `~/.taskbook/ditto-credentials.json`, not here.

```json
{
  "ditto": {
    "appId": "my-taskbook",
    "connect": "peers"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `appId` | `string` | Ditto app / database ID shared by all devices (required) |
| `connect` | `"peers"` \| `"server"` | LAN / P2P mesh, or Ditto Cloud / Big Peer |
| `url` | `string` | Auth / sync URL for `server` mode |
| `provider` | `string` | Auth provider name used with the token in `server` mode |
| `persistenceDir` | `string` | Ditto's local database directory (default `~/.taskbook/ditto`) |
| `collection` | `string` | Collection holding the items |
| `encrypt` | `boolean` | Encrypt payloads client-side (AES-256-GCM) |
| `flushTimeoutMs` | `number` | Bounded wait for a peer after a one-shot CLI write |

See [Ditto Backend](ditto.md) for the full reference.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `TASKBOOK_DIR` | Override taskbook data directory |
| `EDITOR` | External editor for `--note` and `--edit-note` (falls back to `VISUAL`, then `vi`) |

## Data Storage

```
~/.taskbook/
├── storage/
│   └── storage.json      # Active items (JSON)
├── archive/
│   └── archive.json      # Archived items (JSON)
└── credentials.json      # Server credentials (when using sync)
```

### storage.json Format

Items are stored as a JSON object with string IDs as keys:

```json
{
  "1": {
    "_id": 1,
    "_date": "Mon Jan 06 2025",
    "_timestamp": 1736193600000,
    "_description": "Task description",
    "_isStarred": false,
    "_boards": ["My Board"],
    "_isTask": true,
    "isComplete": false,
    "inProgress": false,
    "priority": 1
  }
}
```

This format is compatible with the original Node.js taskbook for easy migration.

## Example Configurations

### Minimal

```json
{}
```

All defaults are used.

### Work Setup

```json
{
  "taskbookDirectory": "~/work",
  "displayCompleteTasks": false,
  "theme": "catppuccin-mocha"
}
```

### With Server Sync

```json
{
  "taskbookDirectory": "~",
  "displayCompleteTasks": true,
  "displayProgressOverview": true,
  "theme": "catppuccin-macchiato",
  "sync": {
    "enabled": true,
    "serverUrl": "https://taskbook.example.com"
  }
}
```

### High Contrast

```json
{
  "theme": "high-contrast",
  "displayProgressOverview": true
}
```
