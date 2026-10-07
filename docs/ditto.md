# Ditto Backend

Taskbook can store its data in [Ditto](https://ditto.com), a peer-to-peer
CRDT database that syncs between devices over LAN, Bluetooth, peer-to-peer
Wi-Fi, or through a Ditto Cloud / self-hosted Big Peer server. Unlike the
[HTTP server backend](sync.md) there is no taskbook server to run: every
device holds a full copy and changes merge whenever devices can see each
other.

The backend is optional and compiled in with a Cargo feature:

```bash
cargo build --release --features ditto
```

The feature pulls in the `dittolive-ditto` crate, whose build script
downloads a prebuilt native library (`libdittoffi`, ~165 MB static archive)
for your target. Point `DITTOFFI_SEARCH_PATH` at a local copy for sandboxed
builds (Nix, offline CI); `DITTO_LOCAL_BUILD=1` forbids the download.

**Toolchain**: Rust 1.85 through 1.97. Rust 1.98 and newer reject a
`#[repr(transparent)]` struct in the SDK's generated bindings
(`dittolive-ditto-sys` 4.14.7 via `safer-ffi` 0.2.0-rc1) with error E0690,
after [rust-lang/rust#155299](https://github.com/rust-lang/rust/pull/155299)
made that check a hard error. Until Ditto ships a fixed SDK, build the
feature with a pinned toolchain:

```bash
rustup toolchain install 1.97.0
cargo +1.97.0 build --release --features ditto
```

The default build (without the feature) is unaffected.

## Setup

1. **Pick an app ID.** Every device that should share data uses the same
   `ditto.appId`. For a LAN-only mesh any string works; Ditto Cloud issues a
   UUID per app.

2. **Configure** `~/.config/taskbook/taskbook.json`:

   ```json
   {
     "sync": { "enabled": true, "backend": "ditto" },
     "ditto": {
       "appId": "my-taskbook",
       "connect": "peers"
     }
   }
   ```

   For Ditto Cloud or a self-hosted Big Peer:

   ```json
   {
     "sync": { "enabled": true, "backend": "ditto" },
     "ditto": {
       "appId": "00000000-0000-0000-0000-000000000000",
       "connect": "server",
       "url": "https://<app-id>.cloud.ditto.live",
       "provider": "development"
     }
   }
   ```

3. **Create the secrets** with

   ```bash
   tb --ditto-init
   ```

   This writes `~/.taskbook/ditto-credentials.json` (mode 0600). On the first
   device it generates the item encryption key and prints it once. In
   `peers` mode it prompts for an offline license token (free, from the
   [Ditto portal](https://portal.ditto.live); Ditto refuses to start sync
   without one), in `server` mode for the auth token. On every other device
   import the same key (and enter the same license token):

   ```bash
   tb --ditto-init --key <base64 key>
   ```

   The file can also be written by hand:

   ```json
   {
     "encryptionKey": "<base64 32 bytes>",
     "licenseToken": "<offline license token, peers mode only>",
     "privateKeyPath": "~/.taskbook/ditto-shared.der",
     "token": "<auth token, server mode only>"
   }
   ```

4. **Migrate existing local data** (first device only):

   ```bash
   tb --migrate
   ```

5. Check with `tb --status`.

## Configuration reference

| Field | Default | Description |
|-------|---------|-------------|
| `ditto.appId` | `""` (required) | Ditto app / database ID shared by all devices |
| `ditto.connect` | `"peers"` | `peers` (LAN / P2P mesh, no account) or `server` (Ditto Cloud or Big Peer) |
| `ditto.url` | unset | Auth / sync URL for `server` mode |
| `ditto.provider` | `"development"` | Authentication provider (webhook) name used with the token in `server` mode |
| `ditto.persistenceDir` | `~/.taskbook/ditto` | Ditto's local database directory |
| `ditto.collection` | `"taskbook_items"` | Collection holding the items |
| `ditto.encrypt` | `true` | Encrypt payloads client-side with AES-256-GCM |
| `ditto.flushTimeoutMs` | `1000` | After a one-shot CLI write, wait up to this long for a peer before exiting; `0` disables |

Set `TB_DITTO_LOG=info` (or `error`, `warn`, `debug`, `verbose`) to see
Ditto's own log output. It is off by default so it cannot corrupt the TUI.

### Peers mode and the shared key

With no `privateKeyPath`, `peers` mode runs as an open development mesh:
any device with the same app ID on the same network can join. For anything
beyond a trusted home network, generate a shared private key (DER) as
described in Ditto's authentication docs and point `privateKeyPath` at it on
every device. Item payloads stay encrypted with the taskbook key either
way, but the shared key is what stops strangers from joining the mesh and
seeing the (encrypted) documents at all.

## How data is stored

Each item is one Ditto document in the configured collection:

```json
{
  "_id": "6f1b…",          // stable identity across devices
  "tbId": 7,               // the id you see in tb
  "archived": false,
  "deleted": false,        // soft-delete tombstone
  "payload": "…",          // item JSON, or base64 AES-256-GCM ciphertext
  "nonce": "…",            // base64 nonce when encrypted
  "updatedAt": 1700000000000
}
```

The payload is a single opaque value, so an item merges last-writer-wins as
a whole and can be encrypted exactly like the HTTP server backend's blobs.
What Ditto (and Ditto Cloud) can see is the id, archived/deleted flags and
timestamps; the description, boards, tags and dates are inside the
encrypted payload. Field-level merging (one device stars an item while
another edits its description) is deliberately not attempted: the later
write wins for that item.

Writes are diffs. `tb` compares the new state against what it last read and
only upserts changed items and tombstones removed ones, inside one Ditto
transaction. Archiving an item therefore tombstones the active document and
creates a new archived one, which is what the whole-map storage contract
implies.

### Id collisions

Taskbook ids are sequential (`max + 1`), so two devices working offline can
both create item 8. Both documents survive the merge. On the next read the
duplicates are sorted deterministically (oldest write keeps its id) and the
rest are renumbered to fresh ids and written back. Nothing is lost, but an
id you just used can change after a sync. Run `tb` again before acting on
an id you saw before devices reconnected.

### One-shot commands and sync

`tb --task "…"` opens Ditto, writes, and exits. Ditto persists the change
locally immediately, but delivering it to peers needs a connection, which
can take a second to establish. The backend therefore waits (bounded by
`flushTimeoutMs`) for a peer to appear plus a short grace period before
exiting. The TUI is long-lived and syncs continuously, and shows changes
from other devices as they arrive.

## Troubleshooting

- **"ditto.encrypt is on but no encryption key is saved"** — run
  `tb --ditto-init` (or `--key <key>` on a second device).
- **"document … is encrypted but ditto.encrypt is off"** (or the reverse) —
  all devices must agree on `ditto.encrypt`; it cannot be changed after data
  exists without clearing the collection.
- **"decryption failed"** — the devices use different encryption keys.
- **"needs an offline license token"** / **"has not yet been activated"** —
  `peers` mode needs `licenseToken` in the credentials file; `tb --ditto-init`
  asks for it.
- **"this build of tb has no Ditto support"** — rebuild with
  `--features ditto`.
- **Devices don't see each other in `peers` mode** — they need the same
  `appId`, the same `privateKeyPath` (or none on both), and a network that
  allows mDNS/multicast. `TB_DITTO_LOG=info tb` shows transport activity.
