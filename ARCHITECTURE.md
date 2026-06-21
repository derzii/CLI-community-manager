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
├── logger.rs        ActivityLogger – SQLite in ~/.local/share/theblackroom/
├── faq.rs           FaqManager – JSON snippets + clipboard copy via xclip/xsel
├── notion.rs        NotionClient – full REST CRUD, dynamic schema, property helpers
├── discord.rs       DiscordClient – bot token, invite gen, kick/ban, member search
├── app.rs           App struct (ALL state), AppEvent enum, key handlers per screen
└── ui/
    ├── mod.rs       Layout (header/footer), colour palette, shared widget helpers
    └── screens.rs   render_* fn for each of the 7 screens + form overlays
```

---

## State Architecture

```
App {
  screen: Screen          // current active screen
  input_mode: InputMode   // Normal | Editing
  tx/rx: mpsc channel     // async bridge: API tasks → TUI

  notion:  Option<NotionClient>   // None until profile configured
  discord: Option<DiscordClient>
  faq:     FaqManager
  logger:  ActivityLogger
  profiles: ProfileManager

  // flat per-screen state (prefix: m_=members, d_=discord,
  //   f_=faq, p_=payments, a_=activity, s_=settings)
}
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

---

## Screens & Key Bindings

| Key | Action |
|-----|--------|
| `1`–`7` | Switch screen |
| `q` | Quit |
| `Esc` | Cancel form / exit Insert mode |
| `Tab` | Next field (Insert mode) |
| `←/→` | Cycle select options in member form |
| `F2` / `s` | Save form |

### Per-screen shortcuts

**Members (2)**
- `r` / `F5` – refresh from Notion
- `a` – add member (opens dynamic form from schema)
- `e` – edit selected
- `d` – archive/delete
- `u` – unarchive/restore
- `/` – search by title

**Discord (3)**
- `Tab` – cycle sections: Invite → Members → Kick/Ban
- `i` – generate invite
- `c` – copy invite URL to clipboard
- `+`/`-` – adjust invite duration
- `k` – kick user (in Kick/Ban section)
- `b` – ban user

**FAQ (4)**
- `c` / `Enter` – copy snippet to clipboard
- `p` – toggle preview pane
- `a` – add snippet
- `e` – edit snippet
- `D` – delete snippet
- `/` – filter search

**Payments (5)**
- `/` or `s` – search member
- `v` – verify payment (auto-detects payment field in schema)

**Activity (6)**
- `r` / `F5` – reload
- `/` – filter log

**Settings (7)**
- `a` – add profile
- `e` – edit selected profile
- `d` – delete profile
- `Enter` – set as active profile

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
| formula/rollup/created_* | ✓ read | skipped in forms |

---

## Data Locations (Linux)

| Data | Path |
|------|------|
| Profiles | `~/.config/theblackroom/profiles.toml` |
| FAQ snippets | `~/.config/theblackroom/snippets.json` |
| Activity log | `~/.local/share/theblackroom/activity.db` |

---

## Build Fix (Rust 1.75 Compatibility)

Rust 1.75 ships with Ubuntu 24 apt. Some newer crate versions require
`edition = "2024"` (Cargo 1.85+). Workaround: pin transitive deps
in `Cargo.toml`. See the `[dependencies]` section – do NOT remove
the `indexmap`, `url`, or `idna` pins.

To upgrade Rust properly on Linux Mint:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup update stable   # gets 1.85+
```
Then remove the explicit version pins and use relaxed version ranges.

---

## Continuation Prompts (copy-paste for new chats)

### "Add a new screen"
> "I'm continuing TheBlackroom (Rust TUI). Add a screen called X.
> Screens live in `src/ui/screens.rs` as `fn render_X(f, area, app)`.
> Add the variant to `Screen` enum in `src/app.rs`, wire key `8` to it
> in `handle_key()`, and add a tab entry in `render_header()` in
> `src/ui/mod.rs`. Here's the current app.rs: [paste]"

### "Fix a bug in members screen"
> "TheBlackroom Rust TUI. Bug in members screen. Here's
> src/app.rs key_members() and src/ui/screens.rs render_members(): [paste]"

### "Add a new Notion property type"
> "In src/notion.rs, add support for property type 'files'.
> Handle it in extract_value() and build_prop(). Here's notion.rs: [paste]"

### "Improve Discord screen"
> "Add role assignment to the Discord screen in TheBlackroom.
> Discord client is in src/discord.rs, screen renderer in
> src/ui/screens.rs render_discord(), key handler in src/app.rs
> key_discord(). Here's the relevant code: [paste]"
