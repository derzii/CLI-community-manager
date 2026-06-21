use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub id: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created: String,
}

impl Snippet {
    pub fn new(title: String, content: String, tags: Vec<String>) -> Self {
        Snippet {
            id: Uuid::new_v4().to_string(),
            title,
            content,
            tags,
            created: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Store { snippets: Vec<Snippet> }

pub struct FaqManager {
    path: PathBuf,
    pub snippets: Vec<Snippet>,
}

impl FaqManager {
    pub fn load() -> Result<Self> {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("theblackroom");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("snippets.json");
        let snippets = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            serde_json::from_str::<Store>(&raw).unwrap_or_default().snippets
        } else {
            default_snippets()
        };
        Ok(FaqManager { path, snippets })
    }

    pub fn save(&self) -> Result<()> {
        let s = Store { snippets: self.snippets.clone() };
        std::fs::write(&self.path, serde_json::to_string_pretty(&s)?)?;
        Ok(())
    }

    pub fn add(&mut self, title: String, content: String, tags: Vec<String>) -> Result<()> {
        self.snippets.push(Snippet::new(title, content, tags));
        self.save()
    }

    pub fn update(&mut self, id: &str, title: String, content: String, tags: Vec<String>) -> Result<()> {
        if let Some(s) = self.snippets.iter_mut().find(|s| s.id == id) {
            s.title = title; s.content = content; s.tags = tags;
        }
        self.save()
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        self.snippets.retain(|s| s.id != id);
        self.save()
    }

    pub fn filtered(&self, q: &str) -> Vec<usize> {
        let q = q.to_lowercase();
        self.snippets.iter().enumerate()
            .filter(|(_, s)| q.is_empty() ||
                s.title.to_lowercase().contains(&q) ||
                s.content.to_lowercase().contains(&q) ||
                s.tags.iter().any(|t| t.to_lowercase().contains(&q)))
            .map(|(i, _)| i)
            .collect()
    }
}

/// Copy text to clipboard via xclip (standard on Linux Mint / X11).
/// Falls back silently on error; caller shows the text in the TUI instead.
pub fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let progs = ["xclip", "xsel"];
    for prog in &progs {
        let args: &[&str] = if *prog == "xclip" {
            &["-selection", "clipboard"]
        } else {
            &["--clipboard", "--input"]
        };
        if let Ok(mut child) = Command::new(prog).args(args).stdin(Stdio::piped()).spawn() {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}

fn default_snippets() -> Vec<Snippet> {
    vec![
        Snippet::new(
            "Welcome".into(),
            "Welcome to TheBlackroom! 🖤 Please complete onboarding by confirming your payment.".into(),
            vec!["welcome".into(), "onboarding".into()],
        ),
        Snippet::new(
            "Payment request".into(),
            "Hi! To finish your membership, send a payment screenshot and we'll verify within a few hours.".into(),
            vec!["payment".into()],
        ),
        Snippet::new(
            "Discord invite".into(),
            "Here's your personal Discord invite (valid 24 h, single-use): [INVITE_LINK]".into(),
            vec!["discord".into(), "invite".into()],
        ),
        Snippet::new(
            "Already a member?".into(),
            "Looks like you're already in our database! DM me your registered name/email to look you up.".into(),
            vec!["faq".into()],
        ),
        Snippet::new(
            "Membership info".into(),
            "Membership is €XX/month. It includes access to our Discord, exclusive content, and weekly sessions. Interested?".into(),
            vec!["faq".into(), "info".into()],
        ),
    ]
}
