# TheBlackroom – Architecture Reference

Use this doc when starting a new chat to continue development.
Paste the relevant section + the file(s) you want to work on.

---

## Stack

| Layer | Crate | Purpose |
|-------|-------|---------|
| TUI | `ratatui 0.25` + `crossterm 0.27` | Terminal rendering & input |
| Async | `tokio 1` | Runtime; spawns API tasks |
| HTTP | `reqwest 0.11` | Notion + Discord REST calls |
| Storage | `rusqlite 0.31` (bundled) | Activity log (SQLite) |
| Config | `toml 0.8` | Named profiles in TOML |
| FAQ | `serde_json` | Snippets stored as JSON |

**No Python required.** Every feature uses pure Rust crates.

---

## File Map

```
src/
├── main.rs          Entry point. Terminal lifecycle. Tokio event loop.
├── error.rs         AppError enum (thiserror). type Result<T>.
├── config.rs        ProfileManager – TOML profiles in ~/.config/theblackroom/
│                     Profile holds: credentials, Theme, DatabaseTabConfig,
│                     Vec<AutomationRule>.
│                     DatabaseTabConfig: active_database_id, per-db ColumnConfig
│                     layout, per-db Vec<FilterPreset>.
│                     AutomationRule + sub-types: RuleCondition, RuleAction,
│                     RuleConditionSource, RuleConditionOp, RuleActionType.
│                     FilterPreset: named Notion filter saved per database.
│                     Theme enum: Dark | Solarized | Light (persisted per profile).
├── logger.rs        ActivityLogger – SQLite in ~/.local/share/theblackroom/
│                     conn() is pub(crate) so analytics.rs can run direct queries.
├── analytics.rs     AnalyticsSummary, sparkline() helper.
│                     Extends ActivityLogger with analytics_summary() and
│                     export_log_csv(). NEW MODULE – add `mod analytics;` in main.rs.
├── faq.rs           FaqManager – JSON snippets + clipboard copy via xclip/xsel.
├── notion.rs        NotionClient – full REST CRUD, dynamic schema, property helpers.
│                     `_of`/`_in` variants take an explicit db_id (Database tab).
│                     list_databases()    – /v1/search, database-switcher.
│                     search_global()     – /v1/search across all pages (global search).
│                     fetch_page_body()   – /v1/blocks/{id}/children (read).
│                     append_page_body()  – PATCH /v1/blocks/{id}/children (write).
│                     Supports: relation (read+write) and rollup (read) property types.
├── discord.rs       DiscordClient – bot token, invite gen, kick/ban, member search.
│                     list_roles()        – GET /guilds/{id}/roles.
│                     assign_role()       – PUT /guilds/{id}/members/{uid}/roles/{rid}.
│                     remove_role()       – DELETE …/roles/{rid}.
│                     broadcast_message() – POST /channels/{id}/messages.
│                     send_dm()           – creates DM channel then posts message.
│                     get_audit_log()     – GET /guilds/{id}/audit-logs.
│                     Member struct now includes role_ids: Vec<String>.
├── automation.rs    run_rule()      – executes one AutomationRule against live services.
│                     run_onboarding() – creates Notion page + Discord invite atomically.
│                     NEW MODULE – add `mod automation;` in main.rs.
├── app.rs           App struct (ALL state), AppEvent enum, key handlers per screen.
└── ui/
    ├── mod.rs       Layout (header/footer), colour palette, theme accent, shared helpers.
    └── screens.rs   render_* fn for each of the 10 screens + all overlays.
```

---

## Screens

| Key | Screen | Description |
|-----|--------|-------------|
| `1` | Dashboard | Status overview + quick reference cheatsheet |
| `2` | Members | Notion database CRUD with dynamic schema |
| `3` | Discord | 5-section panel: Invite / Members / Kick+Ban / Roles / Broadcast |
| `4` | FAQ | Snippet library with clipboard copy + preview pane |
| `5` | Payments | Member payment search + field verification |
| `6` | Activity | Scrollable, filterable SQLite log |
| `7` | Settings | Profile CRUD + theme switcher |
| `8` | Database | Generic, configurable view over any Notion database |
| `9` | Analytics | Aggregated stats + ASCII sparkline + CSV export |
| `0` | Automation | Sync rule engine + onboarding pipeline |

---

## State Architecture

```
App {
  screen: Screen          // current active screen (10 variants)
  input_mode: InputMode   // Normal | Editing
  tx/rx: mpsc channel     // async bridge: API tasks → TUI

  notion:  Option<NotionClient>
  discord: Option<DiscordClient>
  faq:     FaqManager
  logger:  ActivityLogger
  profiles: ProfileManager

  // ── Track A: UX overlays ───────────────────────────────────────────
  show_help:     bool            // ? overlay
  show_palette:  bool            // : command palette
  palette_input: String
  palette_sel:   usize
  undo_stack:    Vec<UndoAction> // max 20; Ctrl+Z pops and re-issues API call

  // ── Track G: Global search overlay ────────────────────────────────
  gs_open:    bool
  gs_input:   String
  gs_results: Vec<SearchResult>  // source-tagged, merged from 4 async tasks
  gs_sel:     usize
  gs_loading: bool

  // ── per-screen state (prefixed) ────────────────────────────────────
  // m_  = members   d_  = discord   f_  = faq    p_  = payments
  // a_  = activity  s_  = settings  db_ = database
  // an_ = analytics auto_ = automation
}
```

### Profile (`config.rs`)

```rust
Profile {
  name, notion_api_key, notion_database_id,
  discord_bot_token, discord_guild_id, discord_default_channel_id,
  extra: HashMap<String,String>,   // arbitrary power-user KV
  theme: Theme,                    // Dark | Solarized | Light
  database_tab: DatabaseTabConfig, // #[serde(default)] – back-compat
  automation_rules: Vec<AutomationRule>, // #[serde(default)]
}
```

Stored at `~/.config/theblackroom/profiles.toml`. All new fields carry
`#[serde(default)]` so profiles written before the expansion still load.

### Database tab state (`db_*`)

```
db_database_id: Option<String>
db_db_name:     String
db_schema:      Option<Schema>
db_columns:     Vec<ColumnConfig>   // visible / editable flags + order
db_rows:        Vec<Page>
db_mode:        DbMode              // Browse | ConfigureColumns | SwitchDatabase
                                    // | EditCell | ViewBody | FilterPresets
db_filter_presets: Vec<FilterPreset>
db_bulk_sel:    HashSet<usize>      // row indices selected for bulk ops
db_bulk_mode:   bool
db_page_body:   String              // fetched block content for ViewBody mode
db_body_append_buf: String          // typed text to append as new paragraph
// + table/picker selection indices, search buffer, cell-editor scratch buffer
```

### Automation rule state (`auto_*`)

```
auto_rules:       Vec<AutomationRule>  // loaded from active profile on startup
auto_sel:         usize
auto_form:        Option<AutoRuleForm> // editor overlay
auto_running:     bool
auto_last_result: String
auto_onboard_form: Option<OnboardForm> // onboarding pipeline overlay
```

### Analytics state (`an_*`)

```
an_summary:    Option<AnalyticsSummary>
an_loading:    bool
an_export_msg: String
```

### Async Pattern

```rust
// Spawn a task; result comes back on rx:
let tx = self.tx.clone();
let client = self.notion.clone().unwrap();
tokio::spawn(async move {
    match client.query(None).await {
        Ok(pages) => { let _ = tx.send(AppEvent::MembersLoaded(pages)); }
        Err(e)    => { let _ = tx.send(AppEvent::ErrMsg(e.to_string())); }
    }
});

// Main loop drains rx each frame:
app.drain_events();   // → process() → match AppEvent { ... }
```

Global search spawns **four parallel tasks** (Notion, Discord, FAQ local,
Activity local) whose results are merged incrementally via the same channel:

```rust
AppEvent::GlobalSearchResults(Vec<SearchResult>)  // arrives multiple times
```

Each `SearchResult` carries a `SearchSource` tag (`Notion | Discord | Faq | Activity`)
used by the overlay renderer to colour the `[N]`/`[D]`/`[F]`/`[A]` prefix.

### Undo stack

`UndoAction` has two variants:

```rust
UndoAction::UpdatePage { page_id, old_props }  // cell edit: re-PATCH with old value
UndoAction::CreatePage { page_id }             // new page: archive it
```

Capped at 20 entries. `Ctrl+Z` pops the top entry and re-issues the
appropriate API call via a spawn. Written only on confirmed mutations
(cell submit, member create). Members edit and Discord actions are not
currently on the undo stack – flagged for future work.

### Key routing and special-cased modes

`handle_key()` evaluates guards in this order:

1. Global search overlay (`gs_open`) → `handle_global_search_key()`
2. Command palette overlay (`show_palette`) → delegated through `handle_editing()`
3. Help overlay (`show_help`) → any key dismisses
4. Esc: closes the innermost open form/mode
5. Ctrl+Z: undo
6. `?` / `:` / `` ` `` → open overlays
7. `1`–`0` screen switches
8. `q` quit
9. Per-screen handler

`InputMode::Editing` intercepts **before** the per-screen `key_*` function,
routing to `handle_editing()`. Inside `handle_editing()`, the Database tab's
`EditCell`, `SwitchDatabase`, `ViewBody`, and `FilterPresets` modes are
special-cased first (they need arrow keys while typing), then the
Automation form and Onboarding form overlays, then generic field routing.

The `'s'` / `F(2)` save shortcut in `handle_auto_form_editing()` is
placed **before** the `Char(c)` catch-all arm to avoid being shadowed.

---

## Screens & Key Bindings

### Global (any screen)

| Key | Action |
|-----|--------|
| `1`–`0` | Switch screen |
| `q` | Quit |
| `?` | Help overlay |
| `:` | Command palette |
| `` ` `` | Global search overlay |
| `Ctrl+Z` | Undo last write operation |
| `Esc` | Close form / exit Insert mode |
| `Tab` | Next field (Insert mode) |
| `F2` / `s` | Save form |

### Per-screen shortcuts

**Members (2)**
- `r` / `F5` – refresh from Notion
- `a` – add member (opens dynamic form from schema)
- `e` – edit selected
- `d` – archive/delete
- `u` – unarchive/restore
- `/` – search by title

**Discord (3)** – `Tab` cycles 5 sections

| Section | Keys |
|---------|------|
| Invite | `i`:generate  `c`:copy URL  `+`/`-`:adjust hours |
| Members | `↑↓`:select  `d`:DM selected member |
| Kick/Ban | `k`:kick  `b`:ban  `e`:edit user ID + reason |
| Roles | `↑↓`:select  `a`:assign selected role  `x`:remove  `e`:edit target user ID |
| Broadcast | `m`:send message  `A`:fetch Discord audit log |

**FAQ (4)**
- `c` / `Enter` – copy snippet to clipboard
- `p` – toggle preview pane
- `a` – add snippet
- `e` – edit snippet
- `D` – delete snippet
- `/` – filter search

**Payments (5)**
- `/` or `s` – search member
- `v` – verify payment (auto-detects payment field)

**Activity (6)**
- `r` / `F5` – reload
- `/` – filter log

**Settings (7)**
- `a` – add profile
- `e` – edit selected profile
- `d` – delete profile
- `t` – cycle theme (Dark → Solarized → Light)
- `Enter` – set as active profile

**Database (8)** — generic, configurable view over any Notion database

- `↑↓` – move row · `←→` / `Tab` / `BackTab` – move column (visible only)
- `e` / `Enter` – edit selected cell (no-op if read-only)
- `b` – open inline page body viewer (fetches blocks; type to append a paragraph)
- `f` – open named filter preset picker
- `Space` – toggle bulk-select on current row
- `X` – bulk-archive all selected rows
- `c` – column configurator: `Space` toggle visible, `x` toggle editable, `J`/`K` reorder, `s` save, `Esc` close
- `D` – switch database: type to filter, `↑↓` select, `Enter` choose
- `r` / `F5` – refresh rows
- `s` – save column layout + filter presets to active profile
- `/` – search by title

Inside the cell editor, `←→` cycles select/status options or toggles a
checkbox; free-text fields accept normal typing. `Enter` saves, `Esc` cancels.

**Analytics (9)**
- `r` / `F5` – refresh all stats
- `e` – export activity log to `/tmp/theblackroom_export.csv`

**Automation (0)**
- `↑↓` – navigate rules
- `a` – new rule (opens editor overlay)
- `e` – edit selected rule
- `d` / `D` – delete selected rule
- `Space` – toggle rule enabled/disabled
- `Enter` – run selected rule now
- `R` – run all enabled rules
- `o` – open onboarding pipeline form (name + email + channel → Notion page + Discord invite)

---

## Notion Property Types Supported

| Type | Display | Edit |
|------|---------|------|
| title | ✓ | text input |
| rich_text | ✓ | text input |
| select | ✓ | ←/→ cycle options |
| multi_select | ✓ | comma-separated text |
| status | ✓ | ←/→ cycle options |
| checkbox | ✓ | ←/→ toggle |
| number | ✓ | text input (validated) |
| email | ✓ | text input |
| url | ✓ | text input |
| phone_number | ✓ | text input |
| date | ✓ | text input (YYYY-MM-DD) |
| relation | ✓ read | comma-separated page IDs |
| rollup (number) | ✓ read | — |
| rollup (array) | ✓ read | — |
| formula/created_* | ✓ read | skipped in forms |

---

## Automation Rules

Rules are stored in `Profile.automation_rules` (persisted to `profiles.toml`).
Each rule has one condition and one action:

```toml
[[profiles.automation_rules]]
id          = "rule_1234567890"
name        = "Expire → Remove Role"
enabled     = true

[profiles.automation_rules.condition]
source = "Notion"     # Notion | Discord
field  = "Status"
op     = "Equals"     # Equals | NotEquals | Contains
value  = "Expired"

[profiles.automation_rules.action]
action_type = "RemoveDiscordRole"   # AddDiscordRole | RemoveDiscordRole
                                    # | SetNotionField | SendDiscordMessage
                                    # | LogActivity
target      = "987654321"           # role_id / channel_id / field name
value       = ""                    # message text / field value
field_kind  = ""                    # only for SetNotionField
```

`run_rule()` in `automation.rs` fetches all pages from Notion, evaluates the
condition against each, and fires the action for each match. Discord user ID
is resolved from `"Discord ID"` or `"User ID"` Notion properties. Results
are sent back via `AppEvent::AutomationRuleResult` and written to the log.

---

## Global Search (Track G)

`` ` `` opens the overlay from any screen. On each keystroke, four tasks
are spawned concurrently:

| Source | Method | Result label |
|--------|--------|--------------|
| Notion | `NotionClient::search_global()` | `[N]` |
| Discord | `DiscordClient::search_members()` | `[D]` |
| FAQ | local filter on `FaqManager::snippets` | `[F]` |
| Activity | `ActivityLogger::search()` | `[A]` |

Results arrive as `AppEvent::GlobalSearchResults(Vec<SearchResult>)` and
are appended to `app.gs_results` incrementally. `Enter` on a result jumps
to the relevant screen and sets a status message with the result's title and ID.

---

## Analytics (Track D)

`ActivityLogger::analytics_summary()` runs direct SQL aggregations on the
log (not loaded into memory first). Returns `AnalyticsSummary`:

```rust
AnalyticsSummary {
  total_entries, notion_adds, notion_removes, payment_checks,
  discord_invites, discord_kicks, discord_bans, db_cell_updates,
  growth_by_day: Vec<GrowthPoint>,  // date + count for sparkline
  retention_pct: f64,               // (adds - removes) / adds * 100
}
```

`sparkline(data, width)` in `analytics.rs` maps growth points to Unicode
block characters (`▁▂▃▄▅▆▇█`) scaled to the terminal column count.

CSV export writes to `/tmp/theblackroom_export.csv`. Path is hardcoded;
adapt if you want a configurable export directory.

---

## Data Locations (Linux)

| Data | Path |
|------|------|
| Profiles + rules + column layouts | `~/.config/theblackroom/profiles.toml` |
| FAQ snippets | `~/.config/theblackroom/snippets.json` |
| Activity log | `~/.local/share/theblackroom/activity.db` |
| CSV export | `/tmp/theblackroom_export.csv` |

### profiles.toml shape (full example)

```toml
active = "default"

[[profiles]]
name                        = "default"
notion_api_key              = "secret_..."
notion_database_id          = "abc123..."
discord_bot_token           = "Bot ..."
discord_guild_id            = "111..."
discord_default_channel_id  = "222..."
theme                       = "Dark"

[[profiles.automation_rules]]
id      = "rule_1"
name    = "Expired → Remove Role"
enabled = true
[profiles.automation_rules.condition]
source = "Notion"
field  = "Status"
op     = "Equals"
value  = "Expired"
[profiles.automation_rules.action]
action_type = "RemoveDiscordRole"
target      = "ROLE_ID_HERE"
value       = ""
field_kind  = ""

[profiles.database_tab]
active_database_id = "abc123..."

[profiles.database_tab.columns.abc123...]
# ordered Vec<ColumnConfig>
[profiles.database_tab.columns.abc123...0]
property_name = "Name"
visible       = true
editable      = true

[profiles.database_tab.filter_presets.abc123...]
[[profiles.database_tab.filter_presets.abc123...]]
name      = "Active only"
field     = "Status"
op        = "equals"
value     = "Active"
prop_type = "status"
```

---

## Build Fix (Rust 1.75 Compatibility)

Rust 1.75 ships with Ubuntu 24 apt. Some newer crate versions require
`edition = "2024"` (Cargo 1.85+). Workaround: pin transitive deps
in `Cargo.toml`. Do NOT remove the `indexmap`, `url`, or `idna` pins.

Confirmed working pin set on Rust 1.75.0 (Ubuntu 24 apt):

```toml
idna          = "=0.5.0"
idna_adapter  = "=1.0.0"
url           = "=2.5.0"
indexmap      = "=2.2.6"
litemap       = "=0.7.3"
zerofrom      = "=0.1.5"
openssl       = "=0.10.66"
openssl-sys   = "=0.9.103"
```

`openssl-sys` needs system headers: `apt install libssl-dev pkg-config`.
Switching `reqwest` to `rustls-tls` instead of `native-tls` removes the
openssl pins entirely — worth considering.

To upgrade Rust properly on Linux Mint:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup update stable   # gets 1.85+
```
Then remove the explicit version pins and use relaxed ranges.

---

## main.rs additions required

Two new modules must be declared:

```rust
mod analytics;
mod automation;
```

---
