use crate::api::{CustomField, TaskPriority, TaskStatus, PRIORITY_OPTIONS};
use crate::app::{App, Mode, Pane, SETTINGS_ACTIONS};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Points},
        BarChart, Block, Borders, Clear, List, ListItem, Paragraph,
    },
    Frame,
};

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_status(f, outer[0], app);
    render_panes(f, outer[1], app);
    render_help(f, outer[2]);

    match app.mode {
        Mode::Comment => render_comment_modal(f, area, app),
        Mode::EditTitle => render_title_modal(f, area, app),
        Mode::EditDescription => render_description_modal(f, area, app),
        Mode::EditDueDate => render_due_date_modal(f, area, app),
        Mode::StatusPicker => render_status_picker(f, area, app),
        Mode::PriorityPicker => render_priority_picker(f, area, app),
        Mode::Dashboard => render_dashboard(f, area, app),
        Mode::Settings => render_settings(f, area, app),
        Mode::Search => render_search_overlay(f, area, app),
        Mode::FilterOverlay => render_filter_overlay(f, area, app),
        Mode::AssigneePicker => render_assignee_picker(f, area, app),
        Mode::StatusFilterPicker => render_status_filter_picker(f, area, app),
        Mode::PriorityFilterPicker => render_priority_filter_picker(f, area, app),
        Mode::AssigneeEditor => render_assignee_editor(f, area, app),
        Mode::NewTask => render_new_task_modal(f, area, app),
        Mode::Normal => {}
    }
}

fn render_status(f: &mut Frame, area: Rect, app: &App) {
    let workspace = app
        .workspace
        .as_ref()
        .map(|w| w.name.as_str())
        .unwrap_or("—");

    // ── left side: brand + workspace + status text + progress bar ──
    let mut left = vec![
        Span::styled(
            " clickup-tui ",
            Style::default().fg(Color::Black).bg(Color::Cyan),
        ),
        Span::raw(" "),
        Span::styled(
            workspace.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(app.status.clone(), Style::default().fg(Color::DarkGray)),
    ];
    if let Some((done, total)) = app.bg_progress() {
        let pct = (done * 100) / total.max(1);
        let bar = progress_bar(done, total, 18);
        left.push(Span::raw("  "));
        left.push(Span::styled(bar, Style::default().fg(Color::Cyan)));
        left.push(Span::styled(
            format!(" {pct}%"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    // ── right side: total open task count callout ──
    let task_count = app.total_open_tasks();
    let callout_text = format!(" {task_count} tasks ▾ ");
    let callout_style = if app.topbar_focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };

    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(callout_text.chars().count() as u16)])
        .split(area);

    f.render_widget(Paragraph::new(Line::from(left)), split[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(callout_text, callout_style)))
            .alignment(Alignment::Right),
        split[1],
    );
}

fn progress_bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 || width == 0 {
        return String::new();
    }
    let filled = (done * width) / total;
    let mut s = String::with_capacity(width * 3);
    for i in 0..width {
        if i < filled {
            s.push('▰');
        } else {
            s.push('▱');
        }
    }
    s
}

fn render_help(f: &mut Frame, area: Rect) {
    let help = " ↑ ↓ ← →  navigate    ⏎  select    esc  menu / shortcuts    q  quit ";
    f.render_widget(
        Paragraph::new(help).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_search_overlay(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(80, 75, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Search — ⏎ open  ↑/↓ move  esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    // Query line
    let query_line = Line::from(vec![
        Span::styled("/", Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::raw(app.search_query.clone()),
        Span::styled("█", Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(query_line), layout[0]);

    // Hits
    if app.search_query.is_empty() {
        let hint = Paragraph::new("type to search across loaded spaces, folders, lists, tasks")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, layout[1]);
    } else if app.search_hits.is_empty() {
        let hint = Paragraph::new("no matches")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, layout[1]);
    } else {
        let items: Vec<ListItem> = app
            .search_hits
            .iter()
            .map(|h| {
                let kind_color = match h.kind {
                    crate::app::HitKind::Space => Color::Magenta,
                    crate::app::HitKind::Folder => Color::Blue,
                    crate::app::HitKind::List => Color::Green,
                    crate::app::HitKind::Task => Color::Yellow,
                };
                let mut chars = h.label.splitn(2, "  ");
                let kind_label = chars.next().unwrap_or("");
                let rest = chars.next().unwrap_or("");
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{kind_label:<7}"), Style::default().fg(kind_color)),
                    Span::raw("  "),
                    Span::raw(rest.to_string()),
                ]))
            })
            .collect();
        let widget = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(Color::Cyan)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");
        f.render_stateful_widget(widget, layout[1], &mut app.search_hits_state);
    }

    // Footer
    let pending = app.bg_pending_count();
    let footer = if pending > 0 {
        format!(
            " {} hit{}  ·  loading {} more list{}… ",
            app.search_hits.len(),
            if app.search_hits.len() == 1 { "" } else { "s" },
            pending,
            if pending == 1 { "" } else { "s" }
        )
    } else {
        format!(
            " {} hit{}  ·  workspace fully loaded ",
            app.search_hits.len(),
            if app.search_hits.len() == 1 { "" } else { "s" }
        )
    };
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
        layout[2],
    );
}

/// Render a 3-pane miller-style window. The focused pane sits in the middle
/// when possible; at the edges the window clips to the available levels.
fn render_panes(f: &mut Frame, area: Rect, app: &mut App) {
    const TOTAL: usize = 5;
    let focused = app.focused.index();

    let start = focused.saturating_sub(1);
    let end = (start + 3).min(TOTAL);
    let count = end - start;

    let constraints: Vec<Constraint> = match count {
        3 => vec![
            Constraint::Percentage(22),
            Constraint::Percentage(28),
            Constraint::Percentage(50),
        ],
        2 => vec![Constraint::Percentage(30), Constraint::Percentage(70)],
        _ => vec![Constraint::Percentage(100)],
    };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (slot, pane_idx) in (start..end).enumerate() {
        let rect = cols[slot];
        match pane_idx {
            0 => render_spaces(f, rect, app),
            1 => render_folders(f, rect, app),
            2 => render_lists(f, rect, app),
            3 => render_tasks(f, rect, app),
            4 => render_detail(f, rect, app),
            _ => {}
        }
    }
}

fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let border = if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(border))
}

fn pane_title(base: &str, filter: &str) -> String {
    if filter.is_empty() {
        base.to_string()
    } else {
        format!("{base} · '{filter}'")
    }
}

fn render_spaces(f: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .spaces_view
        .iter()
        .filter_map(|&i| app.spaces.get(i))
        .map(|s| ListItem::new(s.name.clone()))
        .collect();
    let title = pane_title("Spaces", &app.spaces_filter);
    let widget = List::new(items)
        .block(pane_block(&title, app.focused == Pane::Spaces))
        .highlight_style(highlight_style(app.focused == Pane::Spaces))
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, area, &mut app.spaces_state);
}

fn render_folders(f: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .folders_view
        .iter()
        .filter_map(|&i| app.folders.get(i))
        .map(|fe| {
            let count = if fe.list_count > 0 {
                format!("  ({})", fe.list_count)
            } else {
                String::new()
            };
            ListItem::new(format!("{}{count}", fe.name))
        })
        .collect();
    let mut title = pane_title("Folders", &app.folders_filter);
    if app.filter_hide_empty {
        title.push_str(" · with-tasks");
    }
    let widget = List::new(items)
        .block(pane_block(&title, app.focused == Pane::Folders))
        .highlight_style(highlight_style(app.focused == Pane::Folders))
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, area, &mut app.folders_state);
}

fn render_lists(f: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .lists_view
        .iter()
        .filter_map(|&i| app.lists.get(i))
        .map(|l| {
            let count = l
                .task_count
                .map(|c| format!("  ({c})"))
                .unwrap_or_default();
            ListItem::new(format!("{}{count}", l.name))
        })
        .collect();
    let mut title = pane_title("Lists", &app.lists_filter);
    if app.filter_hide_empty {
        title.push_str(" · with-tasks");
    }
    let widget = List::new(items)
        .block(pane_block(&title, app.focused == Pane::Lists))
        .highlight_style(highlight_style(app.focused == Pane::Lists))
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, area, &mut app.lists_state);
}

fn render_tasks(f: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .tasks_view
        .iter()
        .filter_map(|&i| app.tasks.get(i))
        .map(|t| {
            let color = status_color(t.status.as_ref());
            let line = Line::from(vec![
                Span::styled("● ", Style::default().fg(color)),
                Span::raw(t.name.clone()),
            ]);
            ListItem::new(line)
        })
        .collect();
    let mut title = pane_title("Tasks", &app.tasks_filter);
    if let Some(name) = &app.filter_assignee {
        title.push_str(&format!(" · @{name}"));
    }
    let widget = List::new(items)
        .block(pane_block(&title, app.focused == Pane::Tasks))
        .highlight_style(highlight_style(app.focused == Pane::Tasks))
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, area, &mut app.tasks_state);
}

fn render_detail(f: &mut Frame, area: Rect, app: &mut App) {
    let block = pane_block("Detail", app.focused == Pane::Detail);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(task) = app.task_detail.as_ref() else {
        let hint = Paragraph::new("select a task →")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, inner);
        return;
    };

    // Reserve 2 cols for the focus indicator that ratatui's List inserts on
    // every item so values stay visually aligned.
    let wrap_width = inner.width.saturating_sub(2) as usize;
    let label = |s: &'static str| Span::styled(s, Style::default().fg(Color::DarkGray));

    // 1. Title
    let mut title_lines: Vec<Line> = vec![Line::from(label("Title"))];
    for line in wrap_text(&task.name, wrap_width) {
        title_lines.push(Line::from(Span::styled(
            line,
            Style::default().add_modifier(Modifier::BOLD),
        )));
    }
    let title_item = ListItem::new(title_lines);

    // 2. Status
    let status_text = task
        .status
        .as_ref()
        .map(|s| s.status.clone())
        .unwrap_or_else(|| "—".into());
    let status_col = status_color(task.status.as_ref());
    let status_item = ListItem::new(vec![
        Line::from(label("Status")),
        Line::from(vec![
            Span::styled("● ", Style::default().fg(status_col)),
            Span::styled(status_text, Style::default().fg(status_col)),
        ]),
    ]);

    // 3. Assignees
    let assignees_text = if task.assignees.is_empty() {
        "unassigned".to_string()
    } else {
        task.assignees
            .iter()
            .map(|a| a.username.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut assignees_lines: Vec<Line> = vec![Line::from(label("Assignees"))];
    for line in wrap_text(&assignees_text, wrap_width) {
        assignees_lines.push(Line::from(line));
    }
    let assignees_item = ListItem::new(assignees_lines);

    // 4. Due Date
    let due_date_text = format_due_date(task.due_date.as_deref());
    let now_ms = chrono::Utc::now().timestamp_millis();
    let overdue = task
        .due_date
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|due| due < now_ms)
        .unwrap_or(false);
    let mut due_value_spans: Vec<Span> = vec![Span::raw(due_date_text)];
    if overdue {
        due_value_spans.push(Span::styled(
            "  (overdue)",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ));
    }
    let due_date_item = ListItem::new(vec![
        Line::from(label("Due Date")),
        Line::from(due_value_spans),
    ]);

    // 5. Priority
    let (priority_text, priority_col) = format_priority(task.priority.as_ref());
    let priority_item = ListItem::new(vec![
        Line::from(label("Priority")),
        Line::from(vec![
            Span::styled("● ", Style::default().fg(priority_col)),
            Span::styled(priority_text, Style::default().fg(priority_col)),
        ]),
    ]);

    // 6. Description
    let description_text = task
        .description
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(task.text_content.as_deref().filter(|s| !s.is_empty()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "(empty)".into());
    let description_item =
        preview_item("Description", &description_text, wrap_width, 5);

    // 5. Comments
    let comments_label = if app.comments.is_empty() {
        "Comments".to_string()
    } else {
        format!("Comments ({})", app.comments.len())
    };
    let comments_body = if let Some(c) = app.comments.first() {
        let user = c
            .user
            .as_ref()
            .map(|u| u.username.as_str())
            .unwrap_or("?");
        format!("@{user}: {}", c.comment_text)
    } else {
        "(none)".into()
    };
    let mut comments_lines: Vec<Line> = vec![Line::from(Span::styled(
        comments_label,
        Style::default().fg(Color::DarkGray),
    ))];
    let wrapped = wrap_text(&comments_body, wrap_width);
    let total = wrapped.len();
    let take = 4;
    for line in wrapped.into_iter().take(take) {
        comments_lines.push(Line::from(line));
    }
    if total > take {
        comments_lines.push(Line::from(Span::styled(
            "  …",
            Style::default().fg(Color::DarkGray),
        )));
    }
    let comments_item = ListItem::new(comments_lines);

    let mut items = vec![
        title_item,
        status_item,
        assignees_item,
        due_date_item,
        priority_item,
        description_item,
        comments_item,
    ];

    // 6. Custom Fields (only if any)
    if !task.custom_fields.is_empty() {
        let header = format!("Custom Fields ({})", task.custom_fields.len());
        let mut field_lines: Vec<Line> = vec![Line::from(Span::styled(
            header,
            Style::default().fg(Color::DarkGray),
        ))];
        for cf in &task.custom_fields {
            let value_str = format_custom_field(cf);
            let row = format!("{}: {}", cf.name, value_str);
            for wrapped in wrap_text(&row, wrap_width) {
                field_lines.push(Line::from(wrapped));
            }
        }
        items.push(ListItem::new(field_lines));
    }

    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.detail_state);
}

fn format_custom_field(cf: &CustomField) -> String {
    use serde_json::Value;
    if cf.value.is_null() {
        return "(empty)".to_string();
    }
    match cf.field_type.as_str() {
        "text" | "short_text" | "url" | "email" | "phone" => cf
            .value
            .as_str()
            .unwrap_or("(empty)")
            .to_string(),
        "number" | "currency" | "rating" | "automatic_progress" | "manual_progress" => cf
            .value
            .as_f64()
            .map(|n| {
                if n.fract() == 0.0 {
                    format!("{}", n as i64)
                } else {
                    format!("{n}")
                }
            })
            .or_else(|| cf.value.as_str().map(String::from))
            .unwrap_or_default(),
        "checkbox" => match cf.value.as_str() {
            Some("true") => "✓".to_string(),
            Some("false") => "—".to_string(),
            _ => match cf.value.as_bool() {
                Some(true) => "✓".to_string(),
                Some(false) => "—".to_string(),
                _ => format!("{}", cf.value),
            },
        },
        "date" => {
            let ts: i64 = cf
                .value
                .as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| cf.value.as_i64())
                .unwrap_or(0);
            if ts == 0 {
                "(empty)".to_string()
            } else {
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ts)
                    .map(|dt| dt.format("%b %d, %Y").to_string())
                    .unwrap_or_else(|| format!("{ts}"))
            }
        }
        "drop_down" => {
            let idx = cf.value.as_u64().unwrap_or(0) as usize;
            cf.type_config
                .get("options")
                .and_then(|o| o.as_array())
                .and_then(|arr| arr.get(idx))
                .and_then(|opt| opt.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("(unknown option)")
                .to_string()
        }
        "labels" => {
            let options = cf.type_config.get("options").and_then(|o| o.as_array());
            cf.value
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            let id = v.as_str()?;
                            let opts = options?;
                            opts.iter()
                                .find(|opt| {
                                    opt.get("id").and_then(|i| i.as_str()) == Some(id)
                                })
                                .and_then(|opt| {
                                    opt.get("label").and_then(|l| l.as_str())
                                })
                                .map(String::from)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        }
        "users" => cf
            .value
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.get("username").and_then(|n| n.as_str()))
                    .map(String::from)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        _ => match &cf.value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        },
    }
}

fn preview_item<'a>(label: &'static str, text: &str, width: usize, max_lines: usize) -> ListItem<'a> {
    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        label,
        Style::default().fg(Color::DarkGray),
    ))];
    let wrapped = wrap_text(text, width);
    let total = wrapped.len();
    for line in wrapped.into_iter().take(max_lines) {
        lines.push(Line::from(line));
    }
    if total > max_lines {
        lines.push(Line::from(Span::styled(
            "  …",
            Style::default().fg(Color::DarkGray),
        )));
    }
    ListItem::new(lines)
}

/// Greedy word wrap. Long words hard-break by character. Preserves blank
/// paragraphs in the source.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    for paragraph in s.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            if word_len > width {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                let chars: Vec<char> = word.chars().collect();
                for chunk in chars.chunks(width) {
                    out.push(chunk.iter().collect::<String>());
                }
                continue;
            }
            if current.is_empty() {
                current = word.to_string();
            } else if current.chars().count() + 1 + word_len <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn status_color(status: Option<&TaskStatus>) -> Color {
    status
        .and_then(|s| s.color.as_deref())
        .and_then(parse_hex_color)
        .unwrap_or(Color::DarkGray)
}

fn format_due_date(raw: Option<&str>) -> String {
    let Some(s) = raw.filter(|s| !s.is_empty()) else {
        return "—".into();
    };
    let Ok(ts) = s.parse::<i64>() else {
        return s.to_string();
    };
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ts)
        .map(|dt| dt.format("%b %d, %Y").to_string())
        .unwrap_or_else(|| s.to_string())
}

fn format_priority(priority: Option<&TaskPriority>) -> (String, Color) {
    match priority {
        None => ("—".into(), Color::DarkGray),
        Some(p) => {
            let color = p
                .color
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or(Color::DarkGray);
            (p.priority.clone(), color)
        }
    }
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn render_comment_modal(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(70, 30, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" New comment — ⌃⏎ / ⌥⏎ / F2 send  Esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    f.render_widget(&app.comment_buf, inner);
}

fn render_title_modal(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(70, 14, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Rename task — ⏎ save  esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    f.render_widget(&app.title_buf, inner);
}

fn render_description_modal(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(80, 75, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Edit description — ⌃⏎ / ⌥⏎ / F2 save  Esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    f.render_widget(&app.description_buf, inner);
}

fn render_status_picker(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(50, 50, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Set status — j/k pick  ⏎ apply  esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    if app.status_options.is_empty() {
        let para = Paragraph::new("loading…")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(para, inner);
        return;
    }

    let items: Vec<ListItem> = app
        .status_options
        .iter()
        .map(|s| {
            let color = s
                .color
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or(Color::DarkGray);
            let line = Line::from(vec![
                Span::styled("● ", Style::default().fg(color)),
                Span::raw(s.status.clone()),
            ]);
            ListItem::new(line)
        })
        .collect();
    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.status_picker_state);
}

fn render_priority_picker(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(40, 35, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Set priority — j/k pick  ⏎ apply  esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let items: Vec<ListItem> = PRIORITY_OPTIONS
        .iter()
        .map(|(label, _id, color_hex)| {
            let color = parse_hex_color(color_hex).unwrap_or(Color::DarkGray);
            let line = Line::from(vec![
                Span::styled("● ", Style::default().fg(color)),
                Span::raw(*label),
            ]);
            ListItem::new(line)
        })
        .collect();
    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.priority_picker_state);
}

fn render_dashboard(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(90, 80, area);
    f.render_widget(Clear, modal);
    let total = app.total_open_tasks();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Dashboard — {total} open tasks   esc/q close "))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    // Left: open tasks by assignee (top 12)
    let by_person = app.task_counts_by_assignee();
    let people_pairs: Vec<(&str, u64)> = by_person
        .iter()
        .take(12)
        .map(|(n, c)| (n.as_str(), *c))
        .collect();
    let people_chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Open tasks by person "),
        )
        .data(people_pairs.as_slice())
        .bar_width(7)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .label_style(Style::default().fg(Color::White));
    f.render_widget(people_chart, cols[0]);

    // Right: tasks by status — pie chart + legend
    render_status_pie(f, cols[1], app);
}

fn render_settings(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(60, 80, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Settings — clickup-tui v{} — esc close ",
            env!("CARGO_PKG_VERSION")
        ))
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let bold = Style::default().add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);

    // Split into help (top) + actions (middle) + disclaimer (bottom)
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(SETTINGS_ACTIONS.len() as u16 + 2),
            Constraint::Length(2),
        ])
        .split(inner);

    let help_lines = vec![
        Line::from(Span::styled("Navigation", bold)),
        Line::from("  h ← / l → / Tab    move between panes"),
        Line::from("  j ↓ / k ↑          move within pane"),
        Line::from("  /                  global search"),
        Line::from("  f                  filter overlay"),
        Line::from("  ↑ from top row     dashboard callout"),
        Line::raw(""),
        Line::from(Span::styled("Editing", bold)),
        Line::from("  t  title          e  description"),
        Line::from("  s  status         p  priority"),
        Line::from("  d  due date       a  assignees"),
        Line::from("  c  comment        n  new task"),
        Line::from("  r  refresh"),
        Line::raw(""),
        Line::from(Span::styled("Modal save", bold)),
        Line::from("  ⌃⏎ / ⌥⏎ / F2     save & exit modal"),
        Line::from("  esc               cancel"),
        Line::raw(""),
        Line::from(Span::styled("Config", bold)),
        Line::from(Span::styled(
            "  ~/.config/clickup-tui/config.toml",
            dim,
        )),
    ];
    f.render_widget(Paragraph::new(help_lines), layout[0]);

    // Actions list (interactive)
    let items: Vec<ListItem> = SETTINGS_ACTIONS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let prefix = if i == 0 { "↻ " } else { "✕ " };
            let color = if i == 0 { Color::Yellow } else { Color::Red };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(color)),
                Span::raw(*label),
            ]))
        })
        .collect();
    let actions = List::new(items)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(actions, layout[1], &mut app.settings_state);

    // Disclaimer
    let disclaimer = vec![
        Line::from(Span::styled("Independent open-source project.", dim)),
        Line::from(Span::styled(
            "Not affiliated with or endorsed by ClickUp Inc.",
            dim,
        )),
    ];
    f.render_widget(
        Paragraph::new(disclaimer).alignment(Alignment::Center),
        layout[2],
    );
}

const PIE_FALLBACK_COLORS: &[Color] = &[
    Color::Cyan,
    Color::Magenta,
    Color::Yellow,
    Color::Green,
    Color::Blue,
    Color::Red,
    Color::LightGreen,
    Color::LightMagenta,
    Color::LightCyan,
    Color::LightYellow,
];

fn render_status_pie(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Tasks by status ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let raw = app.task_counts_by_status();
    let total: u64 = raw.iter().map(|(_, c, _)| *c).sum();
    if raw.is_empty() || total == 0 {
        let hint = Paragraph::new("no data yet — wait for loading bar to finish")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(hint, inner);
        return;
    }

    // Build segments with colors and angle ranges (counterclockwise from 3 o'clock)
    let mut segments: Vec<(String, u64, Color, f64, f64)> = Vec::with_capacity(raw.len());
    let mut cum: u64 = 0;
    for (idx, (name, count, hex)) in raw.iter().enumerate() {
        let start = (cum as f64 / total as f64) * std::f64::consts::TAU;
        cum += count;
        let end = (cum as f64 / total as f64) * std::f64::consts::TAU;
        let color = hex
            .as_deref()
            .and_then(parse_hex_color)
            .unwrap_or(PIE_FALLBACK_COLORS[idx % PIE_FALLBACK_COLORS.len()]);
        segments.push((name.clone(), *count, color, start, end));
    }

    // Split inner into pie area (top) and legend (bottom)
    let legend_height = (segments.len() as u16).min(inner.height.saturating_sub(3)) + 1;
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(legend_height)])
        .split(inner);
    let pie_area = split[0];
    let legend_area = split[1];

    // Aspect-correct bounds so a unit-radius circle in world coords renders round.
    // Cells are roughly 1:2 (W:H in pixels); Braille dots within a cell are square.
    // World x range over W cells = W pixels; world y range over H cells = 2H pixels.
    // Setting bounds [-a, a] × [-1, 1] where a = W/(2H) makes a unit circle round.
    let cell_w = pie_area.width as f64;
    let cell_h = pie_area.height as f64;
    let aspect = if cell_h > 0.0 {
        cell_w / (2.0 * cell_h)
    } else {
        1.0
    };
    let (xb, yb) = if aspect >= 1.0 {
        ([-aspect, aspect], [-1.0, 1.0])
    } else {
        ([-1.0, 1.0], [-1.0 / aspect, 1.0 / aspect])
    };

    let pie_segments = segments.clone();
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds(xb)
        .y_bounds(yb)
        .paint(move |ctx| {
            // Sample inside the [-1, 1] × [-1, 1] world square — outside that is unused canvas.
            let res = 180usize;
            let mut buckets: Vec<Vec<(f64, f64)>> = vec![Vec::new(); pie_segments.len()];
            for i in 0..res {
                for j in 0..res {
                    let x = (i as f64 / (res - 1) as f64) * 2.0 - 1.0;
                    let y = (j as f64 / (res - 1) as f64) * 2.0 - 1.0;
                    let r2 = x * x + y * y;
                    if r2 > 0.92_f64.powi(2) {
                        continue;
                    }
                    let mut ang = y.atan2(x);
                    if ang < 0.0 {
                        ang += std::f64::consts::TAU;
                    }
                    for (idx, seg) in pie_segments.iter().enumerate() {
                        if ang >= seg.3 && ang < seg.4 {
                            buckets[idx].push((x, y));
                            break;
                        }
                    }
                }
            }
            for (idx, points) in buckets.iter().enumerate() {
                if points.is_empty() {
                    continue;
                }
                ctx.draw(&Points {
                    coords: points,
                    color: pie_segments[idx].2,
                });
            }
        });
    f.render_widget(canvas, pie_area);

    // Legend
    let lines: Vec<Line> = segments
        .iter()
        .map(|(name, count, color, _, _)| {
            let pct = (*count as f64 / total as f64 * 100.0).round() as u64;
            Line::from(vec![
                Span::styled("■ ", Style::default().fg(*color)),
                Span::styled(
                    name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {count}  ({pct}%)"),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), legend_area);
}

fn render_due_date_modal(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(60, 22, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Due date — ⏎ save  esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(&app.due_date_buf, layout[0]);

    let hint = if let Some(err) = &app.due_date_error {
        Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        ))
    } else {
        Line::from(Span::styled(
            "YYYY-MM-DD · today · tomorrow · +7d · clear",
            Style::default().fg(Color::DarkGray),
        ))
    };
    f.render_widget(Paragraph::new(hint), layout[1]);
}

fn render_filter_overlay(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(55, 40, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Filters — ⏎/space toggle  esc close ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let assignee_label = match &app.filter_assignee {
        None => "(any)".to_string(),
        Some(s) if s == "(unassigned)" => "(unassigned)".to_string(),
        Some(s) => format!("@{s}"),
    };
    let status_label = match &app.filter_status {
        None => "(any)".to_string(),
        Some(s) => s.clone(),
    };
    let priority_label = match app.filter_priority {
        None => "(any)".to_string(),
        Some(None) => "(no priority)".to_string(),
        Some(Some(1)) => "Urgent".to_string(),
        Some(Some(2)) => "High".to_string(),
        Some(Some(3)) => "Normal".to_string(),
        Some(Some(4)) => "Low".to_string(),
        Some(Some(_)) => "(any)".to_string(),
    };
    let overdue_label = if app.filter_overdue { "on" } else { "off" };
    let hide_empty_label = if app.filter_hide_empty { "on" } else { "off" };

    let value_style = Style::default().fg(Color::Cyan);
    let items = vec![
        ListItem::new(Line::from(vec![
            Span::raw("Assignee:         "),
            Span::styled(assignee_label, value_style),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Status:           "),
            Span::styled(status_label, value_style),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Priority:         "),
            Span::styled(priority_label, value_style),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Overdue only:     "),
            Span::styled(overdue_label, value_style),
        ])),
        ListItem::new(Line::from(vec![
            Span::raw("Hide empty lists: "),
            Span::styled(hide_empty_label, value_style),
        ])),
        ListItem::new(Line::from(Span::styled(
            "Clear all filters",
            Style::default().fg(Color::DarkGray),
        ))),
    ];
    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.filter_state);
}

fn render_status_filter_picker(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(50, 60, area);
    f.render_widget(Clear, modal);
    let pending = app.bg_pending_count();
    let title = if pending > 0 {
        format!(
            " Filter by status — loading {} more list{}… ",
            pending,
            if pending == 1 { "" } else { "s" }
        )
    } else {
        " Filter by status — j/k pick  ⏎ apply  esc back ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let items: Vec<ListItem> = app
        .status_filter_options
        .iter()
        .map(|s| ListItem::new(Line::from(s.clone())))
        .collect();
    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.status_filter_picker_state);
}

fn render_priority_filter_picker(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(40, 35, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Filter by priority — j/k pick  ⏎ apply  esc back ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    let rows: [(&str, Color); 6] = [
        ("(any)", Color::DarkGray),
        ("Urgent", parse_hex_color("#f50000").unwrap_or(Color::Red)),
        ("High", parse_hex_color("#ffcc00").unwrap_or(Color::Yellow)),
        ("Normal", parse_hex_color("#6fddff").unwrap_or(Color::Cyan)),
        ("Low", parse_hex_color("#d8d8d8").unwrap_or(Color::Gray)),
        ("(no priority)", Color::DarkGray),
    ];
    let items: Vec<ListItem> = rows
        .iter()
        .map(|(label, color)| {
            let line = Line::from(vec![
                Span::styled("● ", Style::default().fg(*color)),
                Span::raw(*label),
            ]);
            ListItem::new(line)
        })
        .collect();
    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.priority_filter_picker_state);
}

fn render_assignee_picker(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(50, 60, area);
    f.render_widget(Clear, modal);
    let pending = app.bg_pending_count();
    let title = if pending > 0 {
        format!(
            " Filter by assignee — loading {} more list{}… ",
            pending,
            if pending == 1 { "" } else { "s" }
        )
    } else {
        " Filter by assignee — j/k pick  ⏎ apply  esc back ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    if app.assignee_options.is_empty() {
        let para = Paragraph::new("(no assignees in loaded data)")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(para, inner);
        return;
    }

    let items: Vec<ListItem> = app
        .assignee_options
        .iter()
        .map(|name| {
            let display = if name.starts_with('(') {
                Span::styled(name.clone(), Style::default().fg(Color::DarkGray))
            } else {
                Span::raw(format!("@{name}"))
            };
            ListItem::new(Line::from(display))
        })
        .collect();
    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.assignee_picker_state);
}

fn render_new_task_modal(f: &mut Frame, area: Rect, app: &App) {
    let modal = centered(70, 14, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" New task — ⏎ create  esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);
    f.render_widget(&app.new_task_buf, inner);
}

fn render_assignee_editor(f: &mut Frame, area: Rect, app: &mut App) {
    let modal = centered(50, 70, area);
    f.render_widget(Clear, modal);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Assignees — space toggle  ⏎ save  esc cancel ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal);
    f.render_widget(block, modal);

    if app.members.is_empty() {
        let para = Paragraph::new("(no workspace members loaded)")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(para, inner);
        return;
    }

    let items: Vec<ListItem> = app
        .members
        .iter()
        .map(|m| {
            let checked = app.assignee_editor_selections.contains(&m.id);
            let mark = if checked { "✓ " } else { "  " };
            let mark_color = if checked {
                Color::Cyan
            } else {
                Color::DarkGray
            };
            ListItem::new(Line::from(vec![
                Span::styled(mark, Style::default().fg(mark_color)),
                Span::raw(m.username.clone()),
            ]))
        })
        .collect();
    let widget = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");
    f.render_stateful_widget(widget, inner, &mut app.assignee_editor_state);
}

fn highlight_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    }
}

fn centered(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}
