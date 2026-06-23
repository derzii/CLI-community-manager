# TheBlackroom

A terminal-based community management tool that acts as a single pane of glass over Notion and Discord. Built in pure Rust — no Python, no Electron, no browser tabs.

```
 ◼ TBR   1:Dash  2:Members  3:Discord  4:FAQ  5:Payments  6:Log  7:Settings  8:Database  9:Analytics  0:Auto
```

---

## What it does

TheBlackroom connects your Notion workspace and Discord server and lets you manage both from a single terminal interface. Rather than switching between browser tabs, you can add members, generate invites, verify payments, run automation rules, and search across everything — all from the keyboard.

It logs every action it takes to a local SQLite database, so you always have a tamper-evident audit trail of what changed and when.

---

## Features

### Member management
Query, add, edit, and archive pages in any Notion database. Forms are generated dynamically from the database schema — no hardcoded fields. Supports title, rich text, select, multi-select, status, checkbox, number, email, URL, phone, date, and relation property types.

### Discord management
Generate time-limited invites, search and list server members, kick and ban users with reasons, assign and revoke roles, broadcast messages to any channel, send DMs to individual members, and pull the guild audit log — all authenticated via a bot token.

### Generic database viewer
Screen 8 is a fully configurable table view over any Notion database your integration can access. Column visibility, order, and editability are persisted per-database in your profile. Supports inline cell editing, page body viewing, named filter presets, and bulk row selection with one-keystroke archive.

### Automation rules engine
Define if-then rules that run on demand or all at once: if a Notion field equals a value, assign or remove a Discord role, set another Notion field, or send a channel message. Rules are stored in your profile. An onboarding pipeline — create Notion page + generate Discord invite + log — runs in a single keystroke.

### Analytics
Member growth sparkline drawn from the SQLite activity log, aggregated counts for adds, removes, payment checks, invites, kicks, and bans, retention percentage, and one-command CSV export.

### Global search
Press `` ` `` from any screen to open a search overlay that queries Notion pages, Discord members, FAQ snippets, and the activity log simultaneously in parallel async tasks. Results are returned with source labels and `Enter` jumps to the relevant screen.

### Command palette
Press `:` from anywhere to open a fuzzy-filtered command palette. Every screen, action, and pipeline is reachable by name — useful when you can't remember a keybinding.

### FAQ / snippet manager
Store and retrieve frequently-used text snippets with tags and a preview pane. Copy to clipboard via `xclip` or `xsel`. Searchable and filterable.

### Payment verification
Search members, pull their Notion record, and mark a payment field — auto-detected from the schema — as verified in one keystroke.

### Activity log
Every write action is logged to `~/.local/share/theblackroom/activity.db` with a timestamp, kind, target, and result. Filterable and searchable from screen 6. The Analytics screen aggregates this log into metrics and sparklines.

---

## Installation

### Prerequisites

- Rust 1.75+ (Ubuntu 24 apt ships 1.75; see below for upgrade instructions)
- `libssl-dev` and `pkg-config` (for the `reqwest` native-TLS backend)
- `xclip` or `xsel` (optional — for clipboard support on Linux)

```bash
sudo apt install libssl-dev pkg-config xclip
```

### Build from source

```bash
git clone https://github.com/yourname/theblackroom
cd theblackroom
cargo build --release
./target/release/tbr
```

### Rust version note

Rust 1.75 (Ubuntu 24 apt) requires pinned transitive dependencies in `Cargo.toml`. These pins are already present in the repo. To use a fully modern Rust toolchain instead:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup update stable   # gets 1.85+
```

Then remove the explicit version pins from `Cargo.toml` if desired.

---

## Configuration

On first launch, press `7` to go to Settings, then `a` to add a profile. You will be prompted for:

| Field | Description |
|-------|-------------|
| Profile name | Any label — you can have multiple profiles |
| Notion API key | `secret_...` from your Notion integration |
| Notion database ID | The ID of your primary member database |
| Discord bot token | Bot token from the Discord developer portal |
| Discord guild ID | Your server's ID (enable developer mode to copy it) |
| Discord default channel ID | Used for invites and broadcasts when no channel is specified |

Profiles are saved to `~/.config/theblackroom/profiles.toml`. You can have multiple profiles (e.g. one per community) and switch between them with `Enter` in Settings.

### Notion integration setup

1. Go to [notion.so/my-integrations](https://www.notion.so/my-integrations) and create a new integration
2. Copy the **Internal Integration Secret** as your API key
3. Open the Notion database you want to manage, click `...` → `Connections` → add your integration
4. Copy the database ID from the URL: `notion.so/workspace/`**`{database-id}`**`?v=...`

### Discord bot setup

1. Go to [discord.com/developers/applications](https://discord.com/developers/applications) and create a new application
2. Under **Bot**, enable **Server Members Intent** and **Message Content Intent**
3. Copy the bot token
4. Invite the bot to your server with scopes: `bot` and permissions: `Manage Roles`, `Kick Members`, `Ban Members`, `Create Instant Invite`, `Send Messages`, `View Audit Log`

---

## Keybindings

### Global (any screen)

| Key | Action |
|-----|--------|
| `1`–`0` | Switch screen (1=Dash, 2=Members, … 9=Analytics, 0=Automation) |
| `?` | Help overlay |
| `:` | Command palette |
| `` ` `` | Global search overlay |
| `Ctrl+Z` | Undo last write operation |
| `q` | Quit |
| `Esc` | Close form / exit insert mode |
| `Tab` | Next field (in forms) |
| `←/→` | Cycle select/status options (in forms) |
| `F2` / `s` | Save form |

### Members `2`

| Key | Action |
|-----|--------|
| `↑/↓` or `j/k` | Navigate list |
| `r` / `F5` | Refresh from Notion |
| `a` | Add member (dynamic form from schema) |
| `e` | Edit selected member |
| `d` | Archive (soft-delete) selected |
| `u` | Restore archived member |
| `/` | Search by title |

### Discord `3` — `Tab` cycles sections

**Invite section**

| Key | Action |
|-----|--------|
| `i` | Generate invite |
| `c` | Copy invite URL to clipboard |
| `+` / `-` | Adjust expiry in hours |
| `e` | Edit channel ID field |

**Members section**

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate member list |
| `d` | Send DM to selected member |

**Kick/Ban section**

| Key | Action |
|-----|--------|
| `k` | Kick user (by ID in the field) |
| `b` | Ban user |
| `e` | Edit user ID / reason fields |

**Roles section**

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate role list |
| `a` | Assign selected role to the user in the field |
| `x` | Remove selected role from the user |
| `e` | Edit target user ID field |

**Broadcast section**

| Key | Action |
|-----|--------|
| `m` | Send broadcast message |
| `A` | Fetch Discord audit log |

### FAQ `4`

| Key | Action |
|-----|--------|
| `c` / `Enter` | Copy snippet to clipboard |
| `p` | Toggle preview pane |
| `a` | Add snippet |
| `e` | Edit selected snippet |
| `D` | Delete selected snippet |
| `/` | Filter by title/content |

### Payments `5`

| Key | Action |
|-----|--------|
| `/` or `s` | Search member |
| `↑/↓` | Navigate results |
| `v` | Verify payment (auto-detects payment field from schema) |
| `Tab` | Cycle status / notes fields |

### Activity Log `6`

| Key | Action |
|-----|--------|
| `↑/↓` or `j/k` | Scroll log |
| `r` / `F5` | Reload |
| `/` | Filter log entries |

### Settings `7`

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate profiles |
| `a` | Add profile |
| `e` | Edit selected profile |
| `d` | Delete profile |
| `Enter` | Set as active profile |
| `t` | Cycle colour theme (Dark → Solarized → Light) |

### Database `8`

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate rows |
| `←/→` / `Tab` / `BackTab` | Navigate visible columns |
| `e` / `Enter` | Edit selected cell |
| `b` | Open page body viewer (inline block content) |
| `f` | Open named filter presets picker |
| `Space` | Toggle bulk selection on current row |
| `X` | Archive all bulk-selected rows |
| `c` | Open column configurator |
| `D` | Switch to a different database |
| `s` | Save column layout and filter presets to profile |
| `r` / `F5` | Refresh rows |
| `/` | Search by title |
| `Ctrl+Z` | Undo last cell edit |

**Column configurator** (`c`):

| Key | Action |
|-----|--------|
| `Space` | Toggle column visibility |
| `x` | Toggle column editability |
| `J` / `K` | Reorder column down/up |
| `s` | Save layout |
| `Enter` / `Esc` | Close configurator |

**Cell editor** (`e`):

| Key | Action |
|-----|--------|
| `←/→` | Cycle select / status / checkbox values |
| Type | Edit free-text fields |
| `Enter` | Save |
| `Esc` | Cancel |

### Analytics `9`

| Key | Action |
|-----|--------|
| `r` / `F5` | Refresh analytics from log |
| `e` | Export activity log as CSV to `/tmp/theblackroom_export.csv` |

### Automation `0`

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate rules |
| `a` | New rule |
| `e` | Edit selected rule |
| `d` | Delete selected rule |
| `Space` | Toggle rule enabled/disabled |
| `Enter` | Run selected rule now |
| `R` | Run all enabled rules |
| `o` | Open onboarding pipeline form |

---

## Automation rules

Each rule has a **condition** and an **action**. When run, the rule fetches all pages from the primary Notion database and executes the action for every page where the condition matches.

**Condition fields:**
- Source: `Notion` (Discord-sourced conditions are planned)
- Field: any property name in your database (e.g. `Status`, `Paid`)
- Operator: `=`, `≠`, `contains`
- Value: the string to compare against

**Action types:**

| Type | Target field | Value field |
|------|-------------|-------------|
| Add Discord Role | Role ID | — |
| Remove Discord Role | Role ID | — |
| Set Notion Field | Property name | New value |
| Send Discord Message | Channel ID | Message text |
| Log Activity | — | — |

For Discord role actions, the rule looks for a property named `Discord ID` or `User ID` on each matching Notion page to find the target user.

Rules are saved to `profiles.toml` and persist across sessions.

---

## Data locations

| Data | Path |
|------|------|
| Profiles + automation rules | `~/.config/theblackroom/profiles.toml` |
| FAQ snippets | `~/.config/theblackroom/snippets.json` |
| Activity log (SQLite) | `~/.local/share/theblackroom/activity.db` |
| CSV export (on demand) | `/tmp/theblackroom_export.csv` |

---

## Colour themes

Three themes are available, toggled with `t` in Settings:

| Theme | Accent |
|-------|--------|
| Dark (default) | Cyan |
| Solarized | Yellow |
| Light | Blue |

The theme is stored per-profile in `profiles.toml`.

---

## Stack

| Layer | Crate | Purpose |
|-------|-------|---------|
| TUI | `ratatui 0.25` + `crossterm 0.27` | Terminal rendering and input |
| Async | `tokio 1` | Runtime; spawns API tasks |
| HTTP | `reqwest 0.11` | Notion + Discord REST calls |
| Storage | `rusqlite 0.31` (bundled) | Activity log (SQLite) |
| Config | `toml 0.8` | Named profiles in TOML |
| FAQ | `serde_json` | Snippets stored as JSON |

No Python. No Node. No web framework. Every feature uses pure Rust crates.

---

## License

MIT
