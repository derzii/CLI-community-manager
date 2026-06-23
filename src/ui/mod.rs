pub mod screens;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use crate::app::{App, Screen};
use crate::config::Theme;

// ── Colour palette (dark theme defaults) ──────────────────────────────────────

pub const C_BORDER: Color = Color::DarkGray;
pub const C_ACTIVE: Color = Color::Cyan;
pub const C_TEXT:   Color = Color::White;
pub const C_DIM:    Color = Color::Gray;
pub const C_OK:     Color = Color::Green;
pub const C_ERR:    Color = Color::Red;
pub const C_WARN:   Color = Color::Yellow;
pub const C_HL:     Color = Color::Cyan;

// Theme-specific accent colours
fn accent_color(theme: &Theme) -> Color {
    match theme { Theme::Dark => Color::Cyan, Theme::Solarized => Color::Yellow, Theme::Light => Color::Blue }
}
fn accent_bg(theme: &Theme) -> Color {
    match theme { Theme::Dark => Color::DarkGray, Theme::Solarized => Color::DarkGray, Theme::Light => Color::Gray }
}

// ── Style helpers ─────────────────────────────────────────────────────────────

pub fn style_active() -> Style { Style::default().fg(C_ACTIVE).add_modifier(Modifier::BOLD) }
pub fn style_sel()    -> Style { Style::default().fg(Color::Black).bg(C_ACTIVE) }
pub fn style_dim()    -> Style { Style::default().fg(C_DIM) }
pub fn style_ok()     -> Style { Style::default().fg(C_OK) }
pub fn style_err()    -> Style { Style::default().fg(C_ERR) }

pub fn block(title: &str, active: bool) -> Block<'_> {
    Block::default()
        .title(Span::styled(
            format!(" {title} "),
            if active { style_active() } else { Style::default().fg(C_DIM) },
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if active { C_ACTIVE } else { C_BORDER }))
}

// ── Main render entry ─────────────────────────────────────────────────────────

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    render_header(f, chunks[0], app);
    screens::render_screen(f, chunks[1], app);
    render_footer(f, chunks[2], app);
}

// ── Header ────────────────────────────────────────────────────────────────────

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let profile_name = app.profiles.active().map(|p| p.name.as_str()).unwrap_or("no profile");
    let theme = app.profiles.active().map(|p| p.theme.clone()).unwrap_or_default();
    let accent = accent_color(&theme);
    let _bg    = accent_bg(&theme); // reserved for future theme-aware backgrounds

    let tabs: &[(&str, Screen)] = &[
        ("1:Dash",      Screen::Dashboard),
        ("2:Members",   Screen::Members),
        ("3:Discord",   Screen::Discord),
        ("4:FAQ",       Screen::Faq),
        ("5:Payments",  Screen::Payments),
        ("6:Log",       Screen::Activity),
        ("7:Settings",  Screen::Settings),
        ("8:Database",  Screen::Database),
        ("9:Analytics", Screen::Analytics),
        ("0:Auto",      Screen::Automation),
    ];

    let mut spans: Vec<Span> = vec![
        Span::styled(" ◼ TBR ", Style::default().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
    ];
    for (label, scr) in tabs {
        let is_cur = &app.screen == scr;
        spans.push(Span::styled(
            format!(" {label} "),
            if is_cur {
                Style::default().fg(accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                style_dim()
            },
        ));
        spans.push(Span::raw(" "));
    }
    // Global search indicator
    if app.gs_open {
        spans.push(Span::styled(" 🔍 SEARCH ", Style::default().fg(Color::Black).bg(C_WARN).add_modifier(Modifier::BOLD)));
    }
    // Right-align: profile + theme
    let right = format!(" [{}@{}]", profile_name, app.profiles.active().map(|p| p.theme.label()).unwrap_or("Dark"));
    spans.push(Span::styled(right, Style::default().fg(C_WARN)));

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Black)),
        area,
    );
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let mode_label = match app.input_mode {
        crate::app::InputMode::Editing => Span::styled(" INSERT ", Style::default().fg(Color::Black).bg(C_WARN).add_modifier(Modifier::BOLD)),
        crate::app::InputMode::Normal  => Span::styled(" NORMAL ", Style::default().fg(Color::Black).bg(C_DIM)),
    };

    // Undo indicator
    let undo_hint = if app.undo_stack.is_empty() {
        String::new()
    } else {
        format!("  ↩ {} undo(s)", app.undo_stack.len())
    };

    let status_color = if app.status.starts_with("Error") { C_ERR } else { C_TEXT };
    let status = Span::styled(format!("  {}{}", app.status, undo_hint), Style::default().fg(status_color));
    let right  = Span::styled("  ?:help  ::palette  `:search  q:quit", style_dim());

    f.render_widget(
        Paragraph::new(Line::from(vec![mode_label, status, right])).style(Style::default().bg(Color::Black)),
        area,
    );
}

// ── Shared widget helpers ─────────────────────────────────────────────────────

/// Compute a centred sub-rect (percentage of parent).
pub fn centred(pct_x: u16, pct_y: u16, r: Rect) -> Rect {
    let popup_w = r.width  * pct_x / 100;
    let popup_h = r.height * pct_y / 100;
    Rect {
        x: r.x + (r.width  - popup_w) / 2,
        y: r.y + (r.height - popup_h) / 2,
        width: popup_w,
        height: popup_h,
    }
}

/// Single-line text input widget.
pub fn input_widget<'a>(value: &'a str, label: &'a str, active: bool) -> Paragraph<'a> {
    let border_style = if active { Style::default().fg(C_ACTIVE) } else { Style::default().fg(C_BORDER) };
    let title_style  = if active { style_active() } else { style_dim() };
    Paragraph::new(Span::raw(value))
        .block(Block::default()
            .title(Span::styled(format!(" {label} "), title_style))
            .borders(Borders::ALL)
            .border_style(border_style))
        .style(Style::default().fg(C_TEXT))
}

/// Dim hint line at the bottom of a screen block.
pub fn hint_line<'a>(hints: &'a str) -> Paragraph<'a> {
    Paragraph::new(Span::styled(hints, style_dim()))
}
