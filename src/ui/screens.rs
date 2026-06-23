use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap},
};

use crate::{
    app::{App, DbMode, Screen, SearchSource, PALETTE_CMDS},
    ui::{
        block, centred, hint_line, input_widget, style_active, style_dim,
        style_err, style_ok, style_sel, C_ACTIVE, C_BORDER, C_DIM, C_ERR,
        C_OK, C_TEXT, C_WARN,
    },
    analytics::sparkline,
};

// ── Router ────────────────────────────────────────────────────────────────────

pub fn render_screen(f: &mut Frame, area: Rect, app: &mut App) {
    match &app.screen {
        Screen::Dashboard  => render_dashboard(f, area, app),
        Screen::Members    => render_members(f, area, app),
        Screen::Discord    => render_discord(f, area, app),
        Screen::Faq        => render_faq(f, area, app),
        Screen::Payments   => render_payments(f, area, app),
        Screen::Activity   => render_activity(f, area, app),
        Screen::Settings   => render_settings(f, area, app),
        Screen::Database   => render_database(f, area, app),
        Screen::Analytics  => render_analytics(f, area, app),
        Screen::Automation => render_automation(f, area, app),
    }

    // Global overlays (drawn on top of any screen)
    if app.show_help     { render_help_overlay(f, area, app); }
    if app.show_palette  { render_palette_overlay(f, area, app); }
    if app.gs_open       { render_global_search_overlay(f, area, app); }
}

// ── Dashboard ─────────────────────────────────────────────────────────────────

fn render_dashboard(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);

    let profile_line = app.profiles.active()
        .map(|p| format!(
            "  Profile : {}\n  Theme   : {}\n  Notion  : {}\n  Discord : {}",
            p.name,
            p.theme.label(),
            if p.has_notion()  { "✓ configured" } else { "✗ not set" },
            if p.has_discord() { "✓ configured" } else { "✗ not set" },
        ))
        .unwrap_or_else(|| "  No profile active. Press 7 → Settings.".into());

    let text = format!(
        "{}\n\n  Members loaded : {}\n  FAQ snippets   : {}\n  Activity log   : {}\n  Auto rules     : {}",
        profile_line,
        app.m_pages.len(), app.faq.snippets.len(), app.a_logs.len(), app.auto_rules.len()
    );

    f.render_widget(
        Paragraph::new(text)
            .block(block("Dashboard", true))
            .style(Style::default().fg(C_TEXT))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    let help_text = [
        " ╔══════════════════════════════════════════╗",
        " ║  GLOBAL            ?:help  ::palette     ║",
        " ║  1-0   Switch screen        `:search all ║",
        " ║  q     Quit               Ctrl+Z:undo    ║",
        " ╟──────────────────────────────────────────╢",
        " ║  MEMBERS (2)                             ║",
        " ║  r    Refresh   a:add   e:edit   d:del   ║",
        " ║  u    Restore   /:search                 ║",
        " ╟──────────────────────────────────────────╢",
        " ║  DISCORD (3) – Tab cycles sections       ║",
        " ║  Invite: i:gen  c:copy  +/-:hours        ║",
        " ║  Roles:  a:assign  x:remove              ║",
        " ║  Broadcast: m:send   A:audit log         ║",
        " ╟──────────────────────────────────────────╢",
        " ║  DATABASE (8)                            ║",
        " ║  ↑↓:row  ←→/Tab:col  e/↵:edit cell      ║",
        " ║  b:page body  f:filter presets           ║",
        " ║  Space:bulk-select  X:bulk-archive       ║",
        " ║  c:columns  D:switch db  s:save          ║",
        " ╟──────────────────────────────────────────╢",
        " ║  ANALYTICS (9)  r:refresh  e:export CSV  ║",
        " ╟──────────────────────────────────────────╢",
        " ║  AUTOMATION (0)                          ║",
        " ║  a:new  e:edit  d:del  Space:toggle      ║",
        " ║  Enter:run  R:run all  o:onboard         ║",
        " ╚══════════════════════════════════════════╝",
    ];
    f.render_widget(
        Paragraph::new(help_text.join("\n"))
            .block(block("Quick Reference  (? for full help)", false))
            .style(style_dim()),
        chunks[1],
    );
}

// ── Members ───────────────────────────────────────────────────────────────────

fn render_members(f: &mut Frame, area: Rect, app: &mut App) {
    let cols: Vec<String> = app.m_schema.as_ref()
        .map(|s| s.editable().map(|p| p.name.clone()).take(5).collect())
        .unwrap_or_else(|| vec!["Name".into(), "Status".into()]);

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

    let n = cols.len().max(1) as u16;
    let widths: Vec<Constraint> = (0..n).map(|_| Constraint::Percentage(100 / n)).collect();
    let header_cells: Vec<Cell> = cols.iter().map(|c| Cell::from(c.as_str()).style(style_active())).collect();
    let header = Row::new(header_cells).height(1).style(Style::default().bg(Color::DarkGray));

    let search_info = if app.m_search_mode || !app.m_search.is_empty() {
        format!(" [search: {}]", app.m_search)
    } else { String::new() };
    let loading = if app.m_loading { " ⟳" } else { "" };
    let title = format!("Members ({}){}{}", app.m_pages.len(), search_info, loading);

    let table = Table::new(rows, &widths)
        .header(header)
        .block(block(&title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut app.m_list);

    if let Some(form) = &app.m_form {
        let popup_area = centred(70, 80, area);
        f.render_widget(Clear, popup_area);
        let title = if form.is_edit { "Edit Member" } else { "Add Member" };
        let pop = Block::default()
            .title(Span::styled(
                format!(" {title} – Tab:next  ←→:cycle  Enter:save  Esc:cancel "),
                style_active(),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(C_ACTIVE));
        f.render_widget(pop, popup_area);
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let constraints: Vec<Constraint> = form.fields.iter().map(|_| Constraint::Length(3)).collect();
        let field_chunks = Layout::default()
            .direction(Direction::Vertical).constraints(constraints).split(inner);
        for (i, field) in form.fields.iter().enumerate() {
            if i >= field_chunks.len() { break; }
            let display_val = if matches!(field.kind.as_str(), "select"|"status") && !field.options.is_empty() {
                format!("◀ {} ▶  ({}/{})", field.options[field.opt_idx], field.opt_idx + 1, field.options.len())
            } else if field.kind == "checkbox" {
                if field.value == "true" || field.value == "✓" { "✓  (←/→ toggle)".into() }
                else { "✗  (←/→ toggle)".into() }
            } else { field.value.clone() };
            f.render_widget(input_widget(&display_val, &field.label, i == form.active), field_chunks[i]);
        }
    }
}

// ── Discord ───────────────────────────────────────────────────────────────────

fn render_discord(f: &mut Frame, area: Rect, app: &mut App) {
    // Section tabs header
    let sections = ["Invite", "Members", "Kick/Ban", "Roles", "Broadcast"];
    let tab_line: Vec<Span> = sections.iter().enumerate().map(|(i, label)| {
        let s = format!(" {} ", label);
        if i == app.d_section { Span::styled(s, style_active().add_modifier(Modifier::UNDERLINED)) }
        else { Span::styled(s, style_dim()) }
    }).collect();
    let tab_para = Paragraph::new(Line::from(tab_line))
        .style(Style::default().bg(Color::Black));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    f.render_widget(tab_para, chunks[0]);

    match app.d_section {
        0 => render_discord_invite(f, chunks[1], app),
        1 => render_discord_members(f, chunks[1], app),
        2 => render_discord_kickban(f, chunks[1], app),
        3 => render_discord_roles(f, chunks[1], app),
        4 => render_discord_broadcast(f, chunks[1], app),
        _ => {}
    }

    f.render_widget(
        hint_line("  Tab:next section  e:edit fields  A:audit log  ?:help"),
        chunks[2],
    );
}

fn render_discord_invite(f: &mut Frame, area: Rect, app: &mut App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let ch_row = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);
    f.render_widget(input_widget(&app.d_channel, "Channel ID (blank=default)", app.d_active_field == 0), ch_row[0]);
    f.render_widget(input_widget(&format!("{}h  (+/-)", app.d_hours), "Expires", false), ch_row[1]);

    let res_row = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)]).split(rows[1]);
    f.render_widget(input_widget(&format!("{} use(s)", app.d_uses), "Max uses", false), res_row[0]);
    let inv_style = if app.d_invite_result.is_empty() { style_dim() } else { style_ok() };
    let inv_text  = if app.d_invite_result.is_empty() { "— press i to generate —".into() } else { app.d_invite_result.clone() };
    f.render_widget(
        Paragraph::new(Span::styled(inv_text, inv_style))
            .block(Block::default().title(" Generated invite ").borders(Borders::ALL).border_style(Style::default().fg(C_BORDER))),
        res_row[1],
    );
    f.render_widget(hint_line("  i:generate  c:copy  e:edit channel  +/-:hours"), rows[2]);
}

fn render_discord_members(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    f.render_widget(input_widget(&app.d_dm_msg, "DM message (d to send to selected)", false), chunks[0]);

    let items: Vec<ListItem> = app.d_discord_members.iter().map(|m| {
        ListItem::new(format!("  {}  @{}  (ID: {})", m.display, m.username, m.user_id))
    }).collect();
    let title = format!("Server Members ({})", app.d_discord_members.len());
    let list = List::new(items)
        .block(block(&title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[1], &mut app.d_members_list);
}

fn render_discord_kickban(f: &mut Frame, area: Rect, app: &mut App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let uid_row = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[0]);
    f.render_widget(input_widget(&app.d_action_input, "User ID", app.d_active_field == 0), uid_row[0]);
    f.render_widget(input_widget(&app.d_reason, "Reason", app.d_active_field == 1), uid_row[1]);
    f.render_widget(hint_line("  k:kick  b:ban  e:edit fields"), rows[2]);
}

fn render_discord_roles(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    f.render_widget(input_widget(&app.d_assign_user, "Target User ID (for assign/remove)", true), chunks[0]);

    let items: Vec<ListItem> = app.d_roles.iter().map(|r| {
        let color_str = if r.color == 0 { "default".to_string() } else { format!("#{:06X}", r.color) };
        ListItem::new(format!("  {:30}  ID: {}  ({})", r.name, r.id, color_str))
    }).collect();
    let title = format!("Guild Roles ({})", app.d_roles.len());
    let list = List::new(items)
        .block(block(&title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[1], &mut app.d_roles_list);
    f.render_widget(hint_line("  a:assign selected role  x:remove selected role  e:edit user ID"), chunks[2]);
}

fn render_discord_broadcast(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(5), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    f.render_widget(input_widget(&app.d_broadcast_channel, "Channel ID (blank=default)", false), chunks[0]);
    f.render_widget(
        Paragraph::new(app.d_broadcast_msg.as_str())
            .block(Block::default().title(" Message (m to send) ").borders(Borders::ALL).border_style(Style::default().fg(C_BORDER)))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );

    // Audit log preview
    let audit_items: Vec<ListItem> = app.d_audit_log.iter().map(|e| {
        ListItem::new(format!(
            "  {:20}  actor:{}  target:{}  {}",
            e.action_label, e.actor_id, e.target_id,
            if e.reason.is_empty() { String::new() } else { format!("({})", e.reason) }
        ))
    }).collect();
    let audit_title = format!("Audit Log ({} entries – A to refresh)", app.d_audit_log.len());
    let list = List::new(audit_items).block(block(&audit_title, false));
    f.render_widget(list, chunks[2]);
    f.render_widget(hint_line("  m:send message  A:fetch audit log  e:edit channel/msg"), chunks[3]);
}

// ── FAQ ───────────────────────────────────────────────────────────────────────

fn render_faq(f: &mut Frame, area: Rect, app: &mut App) {
    let (list_area, preview_area) = if app.f_show_preview {
        let c = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area);
        (c[0], Some(c[1]))
    } else { (area, None) };

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

    if let Some(preview) = preview_area {
        if let Some(&idx) = app.f_filtered.get(app.f_sel) {
            if let Some(s) = app.faq.snippets.get(idx) {
                f.render_widget(
                    Paragraph::new(s.content.as_str())
                        .block(block(&format!("Preview – {}", s.title), false))
                        .style(Style::default().fg(C_TEXT))
                        .wrap(Wrap { trim: false }),
                    preview,
                );
            }
        }
    }

    if let Some(form) = &app.f_form {
        let popup_area = centred(75, 75, area);
        f.render_widget(Clear, popup_area);
        let title = if form.id.is_some() { "Edit Snippet" } else { "New Snippet" };
        f.render_widget(
            Block::default()
                .title(Span::styled(format!(" {title} – Tab:next  F2/s:save  Esc:cancel "), style_active()))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
            .split(inner);
        f.render_widget(input_widget(&form.title, "Title", form.active == 0), rows[0]);
        f.render_widget(
            Paragraph::new(form.content.as_str())
                .block(Block::default()
                    .title(Span::styled(" Content ", if form.active == 1 { style_active() } else { style_dim() }))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if form.active == 1 { C_ACTIVE } else { C_BORDER })))
                .wrap(Wrap { trim: false }),
            rows[1],
        );
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

    f.render_widget(block("Verify Payment", false), chunks[2]);
    let inner = chunks[2].inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let r2 = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3)]).split(inner);
    let r = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)]).split(r2[0]);
    f.render_widget(input_widget(&app.p_status_val, "Status value (if select)", app.p_active_field == 1), r[0]);
    f.render_widget(input_widget(&app.p_notes, "Notes", app.p_active_field == 2), r[1]);

    f.render_widget(hint_line("  /:search  ↑↓:select  v:verify payment  Tab:next field"), chunks[3]);
}

// ── Activity Log ──────────────────────────────────────────────────────────────

fn render_activity(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    f.render_widget(input_widget(&app.a_filter, "Filter (/ to edit, Enter to apply)", app.a_filter_mode), chunks[0]);

    let items: Vec<ListItem> = app.a_logs.iter().map(|e| {
        let ok_span = if e.ok { Span::styled(" ✓ ", style_ok()) } else { Span::styled(" ✗ ", style_err()) };
        let ts     = Span::styled(format!("{} ", e.ts), style_dim());
        let kind   = Span::styled(format!("{:18} ", e.kind), Style::default().fg(C_ACTIVE));
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

    let items: Vec<ListItem> = app.profiles.profiles.iter().enumerate().map(|(i, p)| {
        let mark = if i == app.profiles.active_idx { "★ " } else { "  " };
        let style = if i == app.profiles.active_idx {
            Style::default().fg(C_WARN).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(C_TEXT) };
        ListItem::new(Span::styled(
            format!("{}  {:20}  N:{}  D:{}  Theme:{}  Rules:{}",
                mark, p.name,
                if p.has_notion() { "✓" } else { "✗" },
                if p.has_discord() { "✓" } else { "✗" },
                p.theme.label(), p.automation_rules.len()),
            style,
        ))
    }).collect();

    let list = List::new(items)
        .block(block("Profiles  (↵:activate  a:add  e:edit  d:delete  t:theme)", true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[0], &mut app.s_list);
    f.render_widget(hint_line("  a:add  e:edit  d:delete  ↵:set active  t:cycle theme"), chunks[1]);

    if let Some(form) = &app.s_form {
        let popup_area = centred(70, 85, area);
        f.render_widget(Clear, popup_area);
        let title = if form.is_edit { "Edit Profile" } else { "New Profile" };
        f.render_widget(
            Block::default()
                .title(Span::styled(format!(" {title} – Tab:next  F2/s:save  Esc:cancel "), style_active()))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let labels = crate::app::ProfileForm::field_labels();
        let constraints: Vec<Constraint> = (0..labels.len()).map(|_| Constraint::Length(3)).collect();
        let field_areas = Layout::default()
            .direction(Direction::Vertical).constraints(constraints).split(inner);
        let values = form.field_values();
        for (i, label) in labels.iter().enumerate() {
            if i >= field_areas.len() { break; }
            let display = if i == 1 || i == 3 {
                if values[i].is_empty() { String::new() } else { "●".repeat(values[i].len().min(32)) }
            } else { values[i].to_string() };
            let hint = if i == 1 { " (masked)" } else if i == 3 { " (masked)" } else { "" };
            f.render_widget(
                input_widget(&display, &format!("{label}{hint}"), i == form.active),
                field_areas[i],
            );
        }
    }
}

// ── Database (generic, configurable) ─────────────────────────────────────────

fn render_database(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    // Header: database name + bulk selection info
    let db_label = if app.db_db_name.is_empty() {
        app.db_database_id.clone().unwrap_or_else(|| "none — press D to pick".into())
    } else { app.db_db_name.clone() };
    let search_info = if app.db_search_mode || !app.db_search.is_empty() {
        format!("  [search: {}]", app.db_search)
    } else { String::new() };
    let bulk_info = if app.db_bulk_mode && !app.db_bulk_sel.is_empty() {
        format!("  [{} selected – X:bulk-archive  Space:toggle]", app.db_bulk_sel.len())
    } else { String::new() };
    let loading = if app.db_loading { "  ⟳" } else { "" };
    let header_text = format!(" {db_label}{search_info}{bulk_info}{loading}");

    f.render_widget(
        Paragraph::new(header_text)
            .block(block("Database", app.db_mode == DbMode::Browse))
            .style(Style::default().fg(C_TEXT)),
        chunks[0],
    );

    // Main table
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

    let rows: Vec<Row> = app.db_rows.iter().enumerate().map(|(row_i, page)| {
        let is_bulk = app.db_bulk_sel.contains(&row_i);
        let cells: Vec<Cell> = col_names.iter().map(|name| {
            let val = page.display(name);
            let base = if val == "✓" { Style::default().fg(C_OK) }
                       else if val == "✗" { Style::default().fg(C_ERR) }
                       else { Style::default().fg(C_TEXT) };
            let style = if is_bulk { base.bg(Color::DarkGray) } else { base };
            Cell::from(val).style(style)
        }).collect();
        Row::new(cells)
    }).collect();

    let n = col_names.len().max(1) as u16;
    let widths: Vec<Constraint> = (0..n).map(|_| Constraint::Percentage(100 / n)).collect();
    let title = if col_names.is_empty() {
        "Rows — no visible columns (c:configure)".into()
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
        hint_line("  ↑↓:row  ←→/Tab:col  e/↵:edit  b:body  f:presets  Space:select  c:columns  D:switch  s:save  r:refresh  /:search"),
        chunks[2],
    );

    // ── Overlays ──────────────────────────────────────────────────────────────

    if app.db_mode == DbMode::ConfigureColumns {
        let popup_area = centred(60, 75, area);
        f.render_widget(Clear, popup_area);
        f.render_widget(
            Block::default()
                .title(Span::styled(
                    " Configure Columns – Space:vis  x:editable  J/K:reorder  s:save  Esc:done ",
                    style_active(),
                ))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let items: Vec<ListItem> = app.db_columns.iter().enumerate().map(|(i, c)| {
            let vis  = if c.visible  { "[x]" } else { "[ ]" };
            let edit = if c.editable { "editable" } else { "read-only" };
            let style = if i == app.db_cfg_sel { style_sel() } else { Style::default().fg(C_TEXT) };
            ListItem::new(format!(" {vis} {:30} ({edit})", c.property_name)).style(style)
        }).collect();
        f.render_widget(List::new(items), inner);
    }

    if app.db_mode == DbMode::SwitchDatabase {
        let popup_area = centred(60, 70, area);
        f.render_widget(Clear, popup_area);
        f.render_widget(
            Block::default()
                .title(Span::styled(" Switch Database – type to filter  ↑↓:select  ↵:choose  Esc:cancel ", style_active()))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(inner);
        f.render_widget(input_widget(&app.db_picker_input, "Filter", true), rows[0]);
        let filtered = app.db_filtered_indices();
        let items: Vec<ListItem> = filtered.iter().enumerate().map(|(i, &idx)| {
            let db = &app.db_available[idx];
            let style = if i == app.db_picker_sel { style_sel() } else { Style::default().fg(C_TEXT) };
            ListItem::new(format!(" {}  ({})", db.title, db.id)).style(style)
        }).collect();
        f.render_widget(
            List::new(items).block(block(&format!("Databases ({})", filtered.len()), false)),
            rows[1],
        );
    }

    if app.db_mode == DbMode::EditCell {
        let cols_vis = app.db_visible_columns();
        if let Some(col) = cols_vis.get(app.db_col_sel) {
            let popup_area = centred(50, 20, area);
            f.render_widget(Clear, popup_area);
            let kind = app.db_edit_kind().unwrap_or_default();
            let opts = app.db_edit_options();
            let display_val = if kind == "checkbox" {
                if app.db_edit_buf == "true" || app.db_edit_buf == "✓" { "✓  (←/→ toggle)".into() }
                else { "✗  (←/→ toggle)".into() }
            } else if !opts.is_empty() && matches!(kind.as_str(), "select"|"status"|"multi_select") {
                format!("◀ {} ▶", app.db_edit_buf)
            } else { app.db_edit_buf.clone() };
            f.render_widget(
                Block::default()
                    .title(Span::styled(format!(" Edit: {} – ↵:save  Esc:cancel ", col.property_name), style_active()))
                    .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
                popup_area,
            );
            let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
            f.render_widget(input_widget(&display_val, &col.property_name, true), inner);
        }
    }

    if app.db_mode == DbMode::ViewBody {
        let popup_area = centred(75, 80, area);
        f.render_widget(Clear, popup_area);
        let page_title = app.db_rows.get(app.db_row_sel)
            .map(|p| p.title()).unwrap_or_default();
        let loading = if app.db_body_loading { " ⟳ loading…" } else { "" };
        f.render_widget(
            Block::default()
                .title(Span::styled(format!(" Body: {page_title}{loading} – type to append  ↵:save  Esc:close "), style_active()))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)])
            .split(inner);
        f.render_widget(
            Paragraph::new(app.db_page_body.as_str())
                .style(Style::default().fg(C_TEXT))
                .wrap(Wrap { trim: false }),
            body_chunks[0],
        );
        f.render_widget(
            input_widget(&app.db_body_append_buf, "Append paragraph (Enter to save)", true),
            body_chunks[1],
        );
    }

    if app.db_mode == DbMode::FilterPresets {
        let popup_area = centred(55, 60, area);
        f.render_widget(Clear, popup_area);
        f.render_widget(
            Block::default()
                .title(Span::styled(" Filter Presets – ↑↓:select  ↵:apply  Esc:cancel ", style_active()))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let items: Vec<ListItem> = app.db_filter_presets.iter().enumerate().map(|(i, p)| {
            let style = if i == app.db_preset_sel { style_sel() } else { Style::default().fg(C_TEXT) };
            ListItem::new(format!("  {}  ({} {} {})", p.name, p.field, p.op, p.value)).style(style)
        }).collect();
        f.render_widget(List::new(items), inner);
    }
}

// ── Analytics (Track D) ───────────────────────────────────────────────────────

fn render_analytics(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // stats panel
            Constraint::Length(6),  // sparkline
            Constraint::Min(0),     // export msg / padding
            Constraint::Length(1),  // hint
        ])
        .split(area);

    if app.an_loading {
        f.render_widget(
            Paragraph::new(" ⟳  Loading analytics…")
                .block(block("Analytics", true))
                .style(Style::default().fg(C_DIM)),
            chunks[0],
        );
        return;
    }

    let summary_text = if let Some(s) = &app.an_summary {
        format!(
            "  Total log entries    : {}\n\
             \n\
             ┌─ Notion ────────────────────────────────┐\n\
             │  Members added      : {:>6}            │\n\
             │  Members removed    : {:>6}            │\n\
             │  Retention          : {:>5.1}%            │\n\
             │  DB cell updates    : {:>6}            │\n\
             │  Payment checks     : {:>6}            │\n\
             └─────────────────────────────────────────┘\n\
             ┌─ Discord ───────────────────────────────┐\n\
             │  Invites generated  : {:>6}            │\n\
             │  Kicks              : {:>6}            │\n\
             │  Bans               : {:>6}            │\n\
             └─────────────────────────────────────────┘",
            s.total_entries,
            s.notion_adds, s.notion_removes, s.retention_pct, s.db_cell_updates,
            s.payment_checks,
            s.discord_invites, s.discord_kicks, s.discord_bans,
        )
    } else {
        "  No data yet. Press r to load.".into()
    };

    f.render_widget(
        Paragraph::new(summary_text)
            .block(block("Analytics", true))
            .style(Style::default().fg(C_TEXT))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    // Sparkline: member growth over time
    let spark_text = if let Some(s) = &app.an_summary {
        if s.growth_by_day.is_empty() {
            "  No growth data in log yet.".into()
        } else {
            let width = (chunks[1].width.saturating_sub(4)) as usize;
            let spark = sparkline(&s.growth_by_day, width);
            let first = s.growth_by_day.first().map(|p| p.date.as_str()).unwrap_or("");
            let last  = s.growth_by_day.last().map(|p| p.date.as_str()).unwrap_or("");
            format!("  {spark}\n\n  {first}  →  {last}\n  (member additions per day, last {} data points)", s.growth_by_day.len())
        }
    } else { String::new() };

    f.render_widget(
        Paragraph::new(spark_text)
            .block(block("Member Growth Sparkline", false))
            .style(Style::default().fg(C_ACTIVE)),
        chunks[1],
    );

    // Export status
    if !app.an_export_msg.is_empty() {
        f.render_widget(
            Paragraph::new(format!("  {}", app.an_export_msg))
                .block(block("Export", false))
                .style(style_ok()),
            chunks[2],
        );
    }

    f.render_widget(hint_line("  r/F5:refresh analytics   e:export log as CSV"), chunks[3]);
}

// ── Automation (Track F) ──────────────────────────────────────────────────────

fn render_automation(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),     // rule list
            Constraint::Length(3),  // last result
            Constraint::Length(1),  // hint
        ])
        .split(area);

    let items: Vec<ListItem> = app.auto_rules.iter().map(|r| {
        let enabled = if r.enabled { Span::styled(" ● ", style_ok()) } else { Span::styled(" ○ ", style_dim()) };
        let name    = Span::styled(format!("{:25} ", r.name), Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD));
        let cond    = Span::styled(
            format!("if {}.{} {} \"{}\" ", r.condition.source.label(), r.condition.field,
                    r.condition.op.label(), r.condition.value),
            style_dim(),
        );
        let action  = Span::styled(
            format!("→ {} ({})", r.action.action_type.label(), r.action.target),
            Style::default().fg(C_ACTIVE),
        );
        let last = Span::styled(
            r.last_result.as_deref().map(|s| format!("  [{}]", &s[..s.len().min(30)]))
                .unwrap_or_default(),
            style_dim(),
        );
        ListItem::new(Line::from(vec![enabled, name, cond, action, last]))
    }).collect();

    let running_suffix = if app.auto_running { " ⟳ running…" } else { "" };
    let auto_title = format!("Automation Rules ({}){}", app.auto_rules.len(), running_suffix);
    let list = List::new(items)
        .block(block(&auto_title, true))
        .highlight_style(style_sel())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[0], &mut app.auto_list);

    f.render_widget(
        Paragraph::new(format!("  {}", if app.auto_last_result.is_empty() { "No runs yet." } else { &app.auto_last_result }))
            .block(block("Last Result", false))
            .style(style_dim()),
        chunks[1],
    );

    f.render_widget(
        hint_line("  a:new  e:edit  d:del  Space:toggle  ↵:run rule  R:run all  o:onboard member"),
        chunks[2],
    );

    // Rule editor overlay
    if let Some(form) = &app.auto_form {
        let popup_area = centred(72, 90, area);
        f.render_widget(Clear, popup_area);
        f.render_widget(
            Block::default()
                .title(Span::styled(
                    " Rule Editor – Tab:next  ←→:cycle options  F2/s:save  Esc:cancel ",
                    style_active(),
                ))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let n = crate::app::AutoRuleForm::field_count();
        let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Length(3)).collect();
        let field_areas = Layout::default()
            .direction(Direction::Vertical).constraints(constraints).split(inner);
        for i in 0..n {
            if i >= field_areas.len() { break; }
            let label = crate::app::AutoRuleForm::field_label(i);
            let val   = form.field_display(i);
            let is_cycle = matches!(i, 1 | 3 | 5 | 9);
            let display  = if is_cycle { format!("◀ {} ▶", val) } else { val };
            f.render_widget(input_widget(&display, label, i == form.active), field_areas[i]);
        }
    }

    // Onboarding form overlay
    if let Some(form) = &app.auto_onboard_form {
        let popup_area = centred(60, 40, area);
        f.render_widget(Clear, popup_area);
        f.render_widget(
            Block::default()
                .title(Span::styled(" Onboard New Member – Tab:next  ↵:run  Esc:cancel ", style_active()))
                .borders(Borders::ALL).border_style(Style::default().fg(C_ACTIVE)),
            popup_area,
        );
        let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(3)])
            .split(inner);
        f.render_widget(input_widget(&form.name, "Member name", form.active == 0), rows[0]);
        f.render_widget(input_widget(&form.email, "Email (optional)", form.active == 1), rows[1]);
        f.render_widget(input_widget(&form.channel, "Discord channel ID (blank=default)", form.active == 2), rows[2]);
    }
}

// ── Help overlay (Track A) ────────────────────────────────────────────────────

fn render_help_overlay(f: &mut Frame, area: Rect, _app: &App) {
    let popup_area = centred(70, 88, area);
    f.render_widget(Clear, popup_area);

    let help = "\
 GLOBAL\n\
   1-0    Switch screen          ?    This overlay\n\
   :      Command palette        `    Global search\n\
   q      Quit               Ctrl+Z   Undo last write\n\
\n\
 MEMBERS (2)\n\
   r/F5   Refresh from Notion    a    Add member\n\
   e      Edit selected          d    Archive/delete\n\
   u      Restore archived       /    Search by title\n\
\n\
 DISCORD (3)  Tab cycles: Invite→Members→Kick→Roles→Broadcast\n\
   Invite:  i:generate  c:copy  +/-:hours\n\
   Members: ↑↓:select   d:DM selected member\n\
   Kick:    k:kick       b:ban   e:edit fields\n\
   Roles:   ↑↓:select   a:assign  x:remove   e:edit user ID\n\
   Broadcast: m:send message  A:fetch audit log\n\
\n\
 FAQ (4)\n\
   c/↵    Copy snippet to clipboard     p    Toggle preview\n\
   a      Add snippet     e    Edit     D    Delete\n\
   /      Search\n\
\n\
 PAYMENTS (5)\n\
   / or s   Search member    v    Verify payment\n\
\n\
 DATABASE (8)\n\
   ↑↓    Row   ←→/Tab  Column (visible only)\n\
   e/↵   Edit cell   b  Page body viewer\n\
   f     Filter presets picker\n\
   Space Bulk-select row   X  Bulk-archive selected\n\
   c     Configure columns (Space:vis  x:editable  J/K:reorder)\n\
   D     Switch database    s  Save layout    r  Refresh\n\
   /     Search by title   Ctrl+Z  Undo last cell edit\n\
\n\
 ANALYTICS (9)\n\
   r/F5   Refresh     e    Export activity log as CSV\n\
\n\
 AUTOMATION (0)\n\
   a    New rule   e  Edit   d  Delete   Space  Toggle enabled\n\
   ↵    Run rule   R  Run all rules\n\
   o    Onboard new member (Notion page + Discord invite)\n\
\n\
 FORMS (any)\n\
   Tab   Next field    ←→   Cycle select options\n\
   F2/s  Save          Esc  Cancel / exit insert\n\
\n\
 Press any key to dismiss";

    f.render_widget(
        Paragraph::new(help)
            .block(Block::default()
                .title(Span::styled(" Help – press any key to dismiss ", style_active()))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(C_ACTIVE)))
            .style(Style::default().fg(C_TEXT)),
        popup_area,
    );
}

// ── Command palette overlay (Track A) ─────────────────────────────────────────

fn render_palette_overlay(f: &mut Frame, area: Rect, app: &App) {
    let popup_area = centred(55, 75, area);
    f.render_widget(Clear, popup_area);
    f.render_widget(
        Block::default()
            .title(Span::styled(" Command Palette – type to filter  ↑↓:select  ↵:run  Esc:close ", style_active()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(C_ACTIVE)),
        popup_area,
    );
    let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(inner);

    f.render_widget(input_widget(&app.palette_input, "Search commands", true), rows[0]);

    let filtered = app.palette_filtered();
    let items: Vec<ListItem> = filtered.iter().enumerate().map(|(i, &idx)| {
        let cmd = &PALETTE_CMDS[idx];
        let style = if i == app.palette_sel { style_sel() } else { Style::default().fg(C_TEXT) };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {:22}", cmd.label), style.add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {}", cmd.description), style_dim()),
        ])).style(style)
    }).collect();
    f.render_widget(
        List::new(items).block(block(&format!("Commands ({})", filtered.len()), false)),
        rows[1],
    );
}

// ── Global search overlay (Track G) ──────────────────────────────────────────

fn render_global_search_overlay(f: &mut Frame, area: Rect, app: &App) {
    let popup_area = centred(75, 78, area);
    f.render_widget(Clear, popup_area);
    f.render_widget(
        Block::default()
            .title(Span::styled(" Global Search – type to search  ↑↓:select  ↵:jump  Esc:close ", style_active()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(C_ACTIVE)),
        popup_area,
    );
    let inner = popup_area.inner(&ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(inner);

    let loading_label = if app.gs_loading { " ⟳" } else { "" };
    f.render_widget(input_widget(&app.gs_input, &format!("Search all sources{loading_label}"), true), rows[0]);

    let items: Vec<ListItem> = app.gs_results.iter().enumerate().map(|(i, r)| {
        let (src_label, src_style) = match r.source {
            SearchSource::Notion   => ("[N]", Style::default().fg(C_ACTIVE)),
            SearchSource::Discord  => ("[D]", Style::default().fg(C_WARN)),
            SearchSource::Faq      => ("[F]", Style::default().fg(C_OK)),
            SearchSource::Activity => ("[A]", Style::default().fg(C_DIM)),
        };
        let row_style = if i == app.gs_sel { style_sel() } else { Style::default().fg(C_TEXT) };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", src_label), src_style),
            Span::styled(format!("{:35} ", r.title), row_style.add_modifier(Modifier::BOLD)),
            Span::styled(r.preview.chars().take(50).collect::<String>(), style_dim()),
        ])).style(row_style)
    }).collect();

    let result_title = format!("Results ({}) – N=Notion  D=Discord  F=FAQ  A=Activity", app.gs_results.len());
    f.render_widget(
        List::new(items).block(block(&result_title, false)),
        rows[1],
    );
}
