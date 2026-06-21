# TheBlackroom (tbr)

Community management TUI for Linux Mint.  
Pure Rust – no Python, no Electron, no server.

```
┌─ ◼ TBR ──1:Dash──2:Members──3:Discord──4:FAQ──5:Payments──6:Log──7:Settings─[default]─┐
│                                                                                          │
│  ▶ Alice Johnson    alice@mail.com    Active    ✓ Paid                                  │
│    Bob Smith        bob@mail.com      Pending   ✗ Unpaid                                │
│    Charlie Davis    charlie@mail.com  Active    ✓ Paid                                  │
│                                                                                          │
└──────────────────────────────────────────────────────── r:refresh  a:add  e:edit  d:del ┘
```

---

## Install

### Option A – pre-built (after release builds)
```bash
cp tbr ~/.local/bin/
chmod +x ~/.local/bin/tbr
```

### Option B – build from source
```bash
# 1. Install Rust (needs 1.75+ for this repo; 1.85+ removes dep pins)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 2. Build
cd theblackroom
cargo build --release

# 3. Install
cp target/release/tbr ~/.local/bin/

# 4. Optional: clipboard support
sudo apt install xclip
```

### Runtime deps
| Tool | Purpose | Required? |
|------|---------|-----------|
| xclip or xsel | Clipboard copy for FAQ snippets & invite URLs | Optional |

---

## First Run

```bash
tbr
```

1. Press **7** (Settings) → **a** to add a profile
2. Fill in:
   - Profile name (e.g. `default`)
   - Notion integration API key (from notion.so/my-integrations)
   - Notion database ID (from the database URL)
   - Discord bot token (from discord.com/developers/applications)
   - Discord guild ID (right-click server → Copy ID)
   - Discord default channel ID (for invite links)
3. Press **F2** or **s** to save
4. Press **Enter** to set it as active
5. Press **2** to open Members – press **r** to load your Notion database

---

## Notion Setup

1. Create an integration at https://notion.so/my-integrations
2. Copy the **Internal Integration Token** (the API key)
3. Open your database in Notion → click ••• → Connections → add your integration
4. Copy the database ID from the URL:
   `https://notion.so/workspace/DATABASE_ID?v=...`

The app auto-reads your schema – any property types you have will appear
in the add/edit form without any code changes.

---

## Discord Setup

1. Go to discord.com/developers/applications → New Application
2. Bot tab → Add Bot → copy the token
3. OAuth2 → URL Generator:
   - Scopes: `bot`
   - Bot Permissions: `Create Instant Invite`, `Kick Members`, `Ban Members`, `View Audit Log`, `Manage Roles` (as needed)
4. Visit the generated URL to invite the bot to your server
5. Enable Developer Mode in Discord (Settings → Advanced) to right-click copy IDs

---

## Config files

All data is stored locally:

| File | Location |
|------|----------|
| Profiles (API keys) | `~/.config/theblackroom/profiles.toml` |
| FAQ snippets | `~/.config/theblackroom/snippets.json` |
| Activity log | `~/.local/share/theblackroom/activity.db` |

Back up `~/.config/theblackroom/` to preserve your setup.
The profiles file is plain TOML – you can edit it directly.

---

## Key bindings (quick ref)

```
Global:  1-7 switch screen   q quit   Esc cancel/normal-mode
Forms:   Tab next field   Shift+Tab prev   ←/→ cycle options   F2/s save
Members: r refresh   a add   e edit   d delete   u restore   / search
Discord: Tab cycle sections   i invite   c copy   k kick   b ban
FAQ:     c/↵ copy   p preview   a add   e edit   D delete   / filter
Pay:     / search   v verify payment
Log:     r refresh   / filter
```

See ARCHITECTURE.md for the full reference and continuation prompts.
