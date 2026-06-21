use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap},
};

use crate::{
    app::{App, DbMode, Screen},
    ui::{
        block, centred, hint_line, input_widget, style_active, style_dim,
        style_ok, style_sel, C_ACTIVE, C_BORDER, C_ERR, C_OK, C_TEXT, C_WARN,
    },
};

pub fn render_screen(f: &mut Frame, area: Rect, app: &mut App) {
    match &app.screen {
        Screen::Dashboard => render_dashboard(f, area, app),
        Screen::Members   => render_members(f, area, app),
        Screen::Discord   => render_discord(f, area, app),
        Screen::Faq       => render_faq(f, area, app),
        Screen::Payments  => render_payments(f, area, app),
        Screen::Activity  => render_activity(f, area, app),
        Screen::Settings  => render_settings(f, area, app),
        Screen::Database  => render_database(f, area, app),
    }
}

// ── Dashboard ─────────────────────────────────────────────────────────────────

fn render_dashboard(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);

    let profile_line = app.profiles.active()
        .map(|p| format!(
            "  Profile : {}\n  Notion  : {}\n  Discord : {}",
            p.name,
            if p.has_notion()  { "✓ configured" } else { "✗ not configured" },
            if p.has_discord() { "✓ configured" } else { "✗ not configured" },
        ))
        .unwrap_or_else(|| "  No profile active. Press 7 to open Settings.".into());

    let notion_info = format!("  Members loaded : {}", app.m_pages.len());
    let faq_info    = format!("  FAQ snippets   : {}", app.faq.snippets.len());
    let log_info    = format!("  Activity logs  : {}", app.a_logs.len());

    let text = format!("{}\n\n{}\n{}\n{}", profile_line, notion_info, faq_info, log_info);

    let para = Paragraph::new(text)
        .block(block("Dashboard", true))
        .style(Style::default().fg(C_TEXT))
        .wrap(Wrap { trim: false });
    f.render_widget(para, chunks[0]);

    let help_text = vec![
        " ╔══════════════════════════════════════╗",
        " ║  GLOBAL SHORTCUTS                    ║",
        " ║  1-8   Switch screen                 ║",
        " ║  q     Quit                          ║",
        " ╟──────────────────────────────────────╢",
        " ║  MEMBERS (2)                         ║",
        " ║  r     Refresh from Notion           ║",
        " ║  a     Add member                    ║",
        " ║  e     Edit selected                 ║",
        " ║  d     Delete/archive                ║",
        " ║  u     Restore archived              ║",
        " ║  /     Search                        ║",
        " ╟──────────────────────────────────────╢",
        " ║  DISCORD (3)                         ║",
        " ║  Tab   Switch section                ║",
        " ║  i     Create invite (invite sect.)  ║",
        " ║  c     Copy invite link              ║",
        " ║  +/-   Adjust invite hours           ║",
        " ╟──────────────────────────────────────╢",
        " ║  FAQ (4)                             ║",
        " ║  c/↵   Copy snippet to clipboard     ║",
        " ║  p     Toggle preview                ║",
        " ║  a     Add snippet                   ║",
        " ║  e     Edit snippet                  ║",
        " ║  D     Delete snippet                ║",
        " ╟──────────────────────────────────────╢",
        " ║  PAYMENTS (5)                        ║",
        " ║  /     Search member                 ║",
        " ║  v     Verify payment                ║",
        " ╟──────────────────────────────────────╢",
        " ║  DATABASE (8)                        ║",
        " ║  ↑↓    Row   ←→/Tab  Column          ║",
        " ║  e/↵   Edit selected cell            ║",
        " ║  c     Configure columns             ║",
        " ║  D     Switch database               ║",
        " ║  s     Save column layout            ║",
        " ║  r     Refresh   /  Search           ║",
        " ╟──────────────────────────────────────╢",
        " ║  FORMS (any)                         ║",
        " ║  Tab   Next field                    ║",
        " ║  ←/→   Cycle select options          ║",
        " ║  F2/s  Save form                     ║",
        " ║  Esc   Cancel / exit insert mode     ║",
        " ╚══════════════════════════════════════╝",
    ];
    let help_para = Paragraph::new(help_text.join("\n"))
        .block(block("Keyboard Reference", false))
        .style(style_dim());
    f.render_widget(help_para, chunks[1]);
}

// ── Members ───────────────────────────────────────────────────────────────────

fn render_members(f: &mut Frame, area: Rect, app: &mut App) {
    // Build column headers from schema
    let cols: Vec<String> = app.m_schema.as_ref()
        .map(|s| s.editable().map(|p| p.name.clone()).take(5).collect())
        .unwrap_or_else(|| vec!["Name".into(), "Status".into()]);

    // Build rows
    let rows: Vec<Row> = app.m_pages.iter().map(|page| {
        let cells: Vec<Cell> = cols.iter().map(|col| {
            let val = page.display(col);
            let style = if val == "✓" { Style::default().fg(C_OK) }
                        else if val == "✗" { Style::default().fg(C_ERR) }
                        else { Style::default().fg(C_TEXT) };
            Cell::from(val).style(style)
        }).collect();
        Row::new(cells)
    }).collect();

    // Equal-width columns
    let n = cols.len().max(1) as u16;
    let widths: Vec<Constraint> = (0..n).map(|_| Constraint::Percentage(100 / n)).collect();

    let header_cells: Vec<Cell> = cols.iter()
        .map(|c| Cell::from(c.as_str()).style(style_active()))
        .collect();
    let header = Row::new(header_cells).height(1).style(Style::default().bg(Color::DarkGray));

    let search_info = if app.m_search_mode || !app.m_search.is_empty() {
        format!(" [search: {}]", app.m_search)
    } else {
        String::new()
    };
    let loading_suffix = if app.m_loading { " ⟳ Loading..." } else { "" };
    let title = format!("Members ({}){}{}", app.m_pages.len(), search_info, loading_suffix);

    let table = Table::new(rows, &widths)
        .header(header)
        .block(block(&title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, area, &mut app.m_list);

    // Overlay: member form
    if let Some(form) = &app.m_form {
        let popup_area = centred(70, 80, area);
        f.render_widget(Clear, popup_area);

        let title = if form.is_edit { "Edit Member" } else { "Add Member" };
        let n = form.fields.len();
        // Each field = 3 lines (border=1 + content=1 + gap=1), plus title bar 2 + footer 2
        let needed_h = (n as u16 * 3 + 4).min(popup_area.height);

        let popup = Block::default()
            .title(Span::styled(format!(" {title} – Tab:next  ←→:cycle  Enter:save  Esc:cancel "), style_active()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(C_ACTIVE));
        f.render_widget(popup, popup_area);

        let inner = Rect {
            x: popup_area.x + 1, y: popup_area.y + 1,
            width: popup_area.width.saturating_sub(2),
            height: popup_area.height.saturating_sub(2),
        };

        let constraints: Vec<Constraint> = form.fields.iter()
            .map(|_| Constraint::Length(3)).collect();
        let field_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        for (i, field) in form.fields.iter().enumerate() {
            if i >= field_chunks.len() { break; }
            let is_active = i == form.active;
            let display_val = if (field.kind == "select" || field.kind == "status")
                    && !field.options.is_empty() {
                let opt = &field.options[field.opt_idx];
                format!("◀ {} ▶  ({}/{})", opt, field.opt_idx + 1, field.options.len())
            } else if field.kind == "checkbox" {
                if field.value == "true" || field.value == "✓" { "✓  (←/→ toggle)".into() }
                else { "✗  (←/→ toggle)".into() }
            } else {
                field.value.clone()
            };
            let w = input_widget(&display_val, &field.label, is_active);
            f.render_widget(w, field_chunks[i]);
        }
    }
}

// ── Discord ───────────────────────────────────────────────────────────────────

fn render_discord(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0), Constraint::Length(10)])
        .split(area);

    // ── Invite section ────────────────────────────────────────────────────────
    let inv_active = app.d_section == 0;
    let inv_block = block("Invite Generator  [Tab→Members]", inv_active);
    f.render_widget(inv_block.clone(), chunks[0]);
    let inner = chunks[0].inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(1)])
        .split(inner);

    // Channel input + result
    let ch_rows = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);
    f.render_widget(input_widget(&app.d_channel, "Channel ID (blank=default)", inv_active && app.d_active_field == 0), ch_rows[0]);
    let hours_str = format!("{}h  (+/- to change)", app.d_hours);
    f.render_widget(input_widget(&hours_str, "Expires after", inv_active), ch_rows[1]);

    let uses_str = format!("{} use(s)", app.d_uses);
    let result_rows = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)]).split(rows[1]);
    f.render_widget(input_widget(&uses_str, "Max uses", false), result_rows[0]);
    let inv_style = if app.d_invite_result.is_empty() { style_dim() } else { style_ok() };
    let inv_display = if app.d_invite_result.is_empty() { "— press i to generate —".into() } else { app.d_invite_result.clone() };
    let inv_para = Paragraph::new(Span::styled(inv_display, inv_style))
        .block(Block::default().title(" Generated invite ").borders(Borders::ALL).border_style(Style::default().fg(C_BORDER)));
    f.render_widget(inv_para, result_rows[1]);

    f.render_widget(hint_line("  i:generate  c:copy to clipboard  e:edit channel  +/-:hours"), rows[2]);

    // ── Member list section ───────────────────────────────────────────────────
    let mem_active = app.d_section == 1;
    let items: Vec<ListItem> = app.d_discord_members.iter().map(|m| {
        ListItem::new(format!("  {}  @{}  (ID: {})", m.display, m.username, m.user_id))
    }).collect();
    let mem_title = format!("Server Members ({}) [Tab→Kick/Ban]", app.d_discord_members.len());
    let list = List::new(items)
        .block(block(&mem_title, mem_active))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[1], &mut app.d_members_list);

    // ── Kick/Ban section ──────────────────────────────────────────────────────
    let kb_active = app.d_section == 2;
    let kb_inner = block("Kick / Ban [Tab→Invite]", kb_active);
    f.render_widget(kb_inner, chunks[2]);
    let kb_area = chunks[2].inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let kb_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(1)])
        .split(kb_area);
    let uid_rows = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(kb_rows[0]);
    f.render_widget(input_widget(&app.d_action_input, "User ID", kb_active && app.d_active_field == 3), uid_rows[0]);
    f.render_widget(input_widget(&app.d_reason, "Reason", kb_active && app.d_active_field == 4), uid_rows[1]);
    f.render_widget(hint_line("  k:kick  b:ban  e:edit fields"), kb_rows[2]);
}

// ── FAQ ───────────────────────────────────────────────────────────────────────

fn render_faq(f: &mut Frame, area: Rect, app: &mut App) {
    let (list_area, preview_area) = if app.f_show_preview {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    let list_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(list_area);

    f.render_widget(input_widget(&app.f_search, "Search snippets (/)", app.f_search_mode), list_chunks[0]);

    let items: Vec<ListItem> = app.f_filtered.iter().map(|&idx| {
        let s = &app.faq.snippets[idx];
        let tags = if s.tags.is_empty() { String::new() } else { format!(" [{}]", s.tags.join(",")) };
        ListItem::new(format!("  {}{}", s.title, tags))
    }).collect();

    let snippets_title = format!("Snippets ({}/{})", app.f_filtered.len(), app.faq.snippets.len());
    let list = List::new(items)
        .block(block(&snippets_title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, list_chunks[1], &mut app.f_list);
    f.render_widget(hint_line("  c/↵:copy  p:preview  a:add  e:edit  D:delete  /:search"), list_chunks[2]);

    // Preview pane
    if let Some(preview) = preview_area {
        if let Some(&idx) = app.f_filtered.get(app.f_sel) {
            if let Some(s) = app.faq.snippets.get(idx) {
                let preview_title = format!("Preview – {}", s.title);
                let para = Paragraph::new(s.content.as_str())
                    .block(block(&preview_title, false))
                    .style(Style::default().fg(C_TEXT))
                    .wrap(Wrap { trim: false });
                f.render_widget(para, preview);
            }
        }
    }

    // Snippet form overlay
    if let Some(form) = &app.f_form {
        let popup_area = centred(75, 75, area);
        f.render_widget(Clear, popup_area);
        let title = if form.id.is_some() { "Edit Snippet" } else { "New Snippet" };
        let pop = Block::default()
            .title(Span::styled(format!(" {title} – Tab:next  F2/s:save  Esc:cancel "), style_active()))
            .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE));
        f.render_widget(pop, popup_area);
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
            .split(inner);
        f.render_widget(input_widget(&form.title, "Title", form.active == 0), rows[0]);
        let content_para = Paragraph::new(form.content.as_str())
            .block(Block::default()
                .title(Span::styled(" Content ", if form.active == 1 { style_active() } else { style_dim() }))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if form.active == 1 { C_ACTIVE } else { C_BORDER })))
            .wrap(Wrap { trim: false });
        f.render_widget(content_para, rows[1]);
        f.render_widget(input_widget(&form.tags, "Tags (comma-separated)", form.active == 2), rows[2]);
    }
}

// ── Payments ──────────────────────────────────────────────────────────────────

fn render_payments(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(7), Constraint::Length(1)])
        .split(area);

    f.render_widget(input_widget(&app.p_search, "Search member (/ or s)", app.p_search_mode), chunks[0]);

    // Results list (re-uses m_pages from Notion search)
    let cols: Vec<String> = app.m_schema.as_ref()
        .map(|s| s.editable().map(|p| p.name.clone()).take(4).collect())
        .unwrap_or_else(|| vec!["Name".into(), "Status".into(), "Payment".into()]);
    let n = cols.len().max(1) as u16;
    let widths: Vec<Constraint> = (0..n).map(|_| Constraint::Percentage(100 / n)).collect();
    let header: Vec<Cell> = cols.iter().map(|c| Cell::from(c.as_str()).style(style_active())).collect();
    let rows: Vec<Row> = app.p_results.iter().map(|page| {
        let cells: Vec<Cell> = cols.iter().map(|col| {
            let val = page.display(col);
            let style = if val == "✓" { Style::default().fg(C_OK) }
                        else if val == "✗" { Style::default().fg(C_ERR) }
                        else { Style::default().fg(C_TEXT) };
            Cell::from(val).style(style)
        }).collect();
        Row::new(cells)
    }).collect();
    let results_title = format!("Results ({})", app.p_results.len());
    let table = Table::new(rows, &widths)
        .header(Row::new(header).style(Style::default().bg(Color::DarkGray)))
        .block(block(&results_title, true))
        .highlight_style(style_sel());
    f.render_stateful_widget(table, chunks[1], &mut app.p_res_list);

    // Payment action panel
    let pay_inner = block("Verify Payment", false);
    f.render_widget(pay_inner, chunks[2]);
    let inner = chunks[2].inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let rows2 = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3)])
        .split(inner);
    let r = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)]).split(rows2[0]);
    f.render_widget(input_widget(&app.p_status_val, "Payment status value (if select)", app.p_active_field == 1), r[0]);
    f.render_widget(input_widget(&app.p_notes, "Notes", app.p_active_field == 2), r[1]);

    f.render_widget(hint_line("  /:search  ↑↓:select result  v:verify/update payment  Tab:next field"), chunks[3]);
}

// ── Activity Log ──────────────────────────────────────────────────────────────

fn render_activity(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    f.render_widget(input_widget(&app.a_filter, "Filter (/ to edit, Enter to apply)", app.a_filter_mode), chunks[0]);

    let items: Vec<ListItem> = app.a_logs.iter().map(|e| {
        let ok_span = if e.ok {
            Span::styled(" ✓ ", Style::default().fg(C_OK))
        } else {
            Span::styled(" ✗ ", Style::default().fg(C_ERR))
        };
        let ts  = Span::styled(format!("{} ", e.ts), style_dim());
        let kind = Span::styled(format!("{:15} ", e.kind), Style::default().fg(C_ACTIVE));
        let target = Span::styled(format!("{:20} ", e.target), Style::default().fg(C_TEXT));
        let detail = Span::styled(e.detail.clone(), style_dim());
        ListItem::new(Line::from(vec![ok_span, ts, kind, target, detail]))
    }).collect();

    let activity_title = format!("Activity Log ({} entries)", app.a_logs.len());
    let list = List::new(items)
        .block(block(&activity_title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[1], &mut app.a_list);

    f.render_widget(hint_line("  r/F5:refresh  /:filter  ↑↓/j/k:scroll"), chunks[2]);
}

// ── Settings ──────────────────────────────────────────────────────────────────

fn render_settings(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    // Profile list
    let items: Vec<ListItem> = app.profiles.profiles.iter().enumerate().map(|(i, p)| {
        let active_mark = if i == app.profiles.active_idx { "★ " } else { "  " };
        let notion_ok   = if p.has_notion()  { "N✓" } else { "N✗" };
        let discord_ok  = if p.has_discord() { "D✓" } else { "D✗" };
        let style = if i == app.profiles.active_idx {
            Style::default().fg(C_WARN).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(C_TEXT)
        };
        ListItem::new(Span::styled(
            format!("{}  {:20}  {}  {}", active_mark, p.name, notion_ok, discord_ok),
            style,
        ))
    }).collect();

    let list = List::new(items)
        .block(block("Profiles  (↵:activate  a:add  e:edit  d:delete)", true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[0], &mut app.s_list);
    f.render_widget(hint_line("  a:add  e:edit  d:delete  ↵:set active"), chunks[1]);

    // Profile form overlay
    if let Some(form) = &app.s_form {
        let popup_area = centred(70, 85, area);
        f.render_widget(Clear, popup_area);
        let title = if form.is_edit { "Edit Profile" } else { "New Profile" };
        let pop = Block::default()
            .title(Span::styled(format!(" {title} – Tab:next  F2/s:save  Esc:cancel "), style_active()))
            .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE));
        f.render_widget(pop, popup_area);
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let field_labels = crate::app::ProfileForm::field_labels();
        let n = field_labels.len();
        let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Length(3)).collect();
        let field_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);
        let values = form.field_values();
        for (i, label) in field_labels.iter().enumerate() {
            if i >= field_areas.len() { break; }
            // Mask secret fields
            let display = if i == 1 || i == 3 {
                if values[i].is_empty() { String::new() } else { "●".repeat(values[i].len().min(32)) }
            } else {
                values[i].to_string()
            };
            let is_active = i == form.active;
            let hint = if i == 1 { " (API key – masked)" } else if i == 3 { " (token – masked)" } else { "" };
            f.render_widget(
                input_widget(&display, &format!("{label}{hint}"), is_active),
                field_areas[i],
            );
        }
    }
}

// ── Database (generic, configurable) ────────────────────────────────────────

fn render_database(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    // ── Header strip: which database, search box, loading state ──────────────
    let db_label = if app.db_db_name.is_empty() {
        app.db_database_id.clone().unwrap_or_else(|| "none — press D to pick a database".into())
    } else {
        app.db_db_name.clone()
    };
    let search_info = if app.db_search_mode || !app.db_search.is_empty() {
        format!("   [search: {}]", app.db_search)
    } else {
        String::new()
    };
    let loading_suffix = if app.db_loading { "   ⟳ loading..." } else { "" };
    let header_text = format!(" {db_label}{search_info}{loading_suffix}");
    let header_para = Paragraph::new(header_text)
        .block(block("Database", app.db_mode == DbMode::Browse))
        .style(Style::default().fg(C_TEXT));
    f.render_widget(header_para, chunks[0]);

    // ── Main table: visible columns, in user-configured order ────────────────
    let cols: Vec<crate::config::ColumnConfig> = app.db_columns.iter().filter(|c| c.visible).cloned().collect();
    let col_names: Vec<String> = cols.iter().map(|c| c.property_name.clone()).collect();

    let header_cells: Vec<Cell> = col_names.iter().enumerate().map(|(i, name)| {
        if i == app.db_col_sel {
            Cell::from(format!("▸{name}")).style(style_sel())
        } else {
            Cell::from(name.as_str()).style(style_active())
        }
    }).collect();
    let header_row = Row::new(header_cells).height(1).style(Style::default().bg(Color::DarkGray));

    let rows: Vec<Row> = app.db_rows.iter().map(|page| {
        let cells: Vec<Cell> = col_names.iter().map(|name| {
            let val = page.display(name);
            let style = if val == "✓" { Style::default().fg(C_OK) }
                        else if val == "✗" { Style::default().fg(C_ERR) }
                        else { Style::default().fg(C_TEXT) };
            Cell::from(val).style(style)
        }).collect();
        Row::new(cells)
    }).collect();

    let n = col_names.len().max(1) as u16;
    let widths: Vec<Constraint> = (0..n).map(|_| Constraint::Percentage(100 / n)).collect();

    let title = if col_names.is_empty() {
        "Rows — no visible columns (press c to configure)".to_string()
    } else {
        format!("Rows ({})", app.db_rows.len())
    };
    let table = Table::new(rows, &widths)
        .header(header_row)
        .block(block(&title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(table, chunks[1], &mut app.db_table);

    f.render_widget(
        hint_line("  ↑↓:row  ←→/Tab:column  e/↵:edit cell  c:configure columns  D:switch db  s:save layout  r:refresh  /:search"),
        chunks[2],
    );

    // ── Column configurator overlay ───────────────────────────────────────────
    if app.db_mode == DbMode::ConfigureColumns {
        let popup_area = centred(60, 75, area);
        f.render_widget(Clear, popup_area);
        let pop = Block::default()
            .title(Span::styled(
                " Configure Columns – Space:visible  x:editable  J/K:reorder  s:save  Enter/Esc:done ",
                style_active(),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(C_ACTIVE));
        f.render_widget(pop, popup_area);
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });

        let items: Vec<ListItem> = app.db_columns.iter().enumerate().map(|(i, c)| {
            let vis = if c.visible { "[x]" } else { "[ ]" };
            let edit = if c.editable { "editable" } else { "read-only" };
            let style = if i == app.db_cfg_sel { style_sel() } else { Style::default().fg(C_TEXT) };
            ListItem::new(format!(" {vis} {:30} ({edit})", c.property_name)).style(style)
        }).collect();
        let list = List::new(items);
        f.render_widget(list, inner);
    }

    // ── Database switcher overlay ─────────────────────────────────────────────
    if app.db_mode == DbMode::SwitchDatabase {
        let popup_area = centred(60, 70, area);
        f.render_widget(Clear, popup_area);
        let pop = Block::default()
            .title(Span::styled(" Switch Database – type to filter, ↑↓ select, Enter:choose, Esc:cancel ", style_active()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(C_ACTIVE));
        f.render_widget(pop, popup_area);
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(inner);
        f.render_widget(input_widget(&app.db_picker_input, "Filter", true), rows[0]);

        let filtered = app.db_filtered_indices();
        let filtered_title = format!("Databases ({})", filtered.len());
        let items: Vec<ListItem> = filtered.iter().enumerate().map(|(row_i, &idx)| {
            let db = &app.db_available[idx];
            let style = if row_i == app.db_picker_sel { style_sel() } else { Style::default().fg(C_TEXT) };
            ListItem::new(format!(" {}  ({})", db.title, db.id)).style(style)
        }).collect();
        let list = List::new(items).block(block(&filtered_title, false));
        f.render_widget(list, rows[1]);
    }

    // ── Cell editor overlay ───────────────────────────────────────────────────
    if app.db_mode == DbMode::EditCell {
        if let Some(col) = cols.get(app.db_col_sel) {
            let popup_area = centred(50, 20, area);
            f.render_widget(Clear, popup_area);
            let kind = app.db_edit_kind().unwrap_or_default();
            let opts = app.db_edit_options();
            let display_val = if kind == "checkbox" {
                if app.db_edit_buf == "true" || app.db_edit_buf == "✓" { "✓  (←/→ toggle)".into() }
                else { "✗  (←/→ toggle)".into() }
            } else if !opts.is_empty() && matches!(kind.as_str(), "select" | "status" | "multi_select") {
                format!("◀ {} ▶", app.db_edit_buf)
            } else {
                app.db_edit_buf.clone()
            };
            let pop = Block::default()
                .title(Span::styled(format!(" Edit: {} – Enter:save  Esc:cancel ", col.property_name), style_active()))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(C_ACTIVE));
            f.render_widget(pop, popup_area);
            let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
            f.render_widget(input_widget(&display_val, &col.property_name, true), inner);
        }
    }
}
