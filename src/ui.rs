use crate::api::Document;
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table, TableState, Tabs, Wrap,
    },
    Frame,
};
use url::Url;

fn get_hostname(url_str: &str) -> String {
    if let Ok(url) = Url::parse(url_str) {
        if let Some(host) = url.host_str() {
            return host.to_string();
        }
    }
    url_str.to_string()
}

pub enum ViewState {
    List,
    Read { doc: Document, content: String },
}

pub fn draw(
    f: &mut Frame,
    view: &ViewState,
    articles: &[Document],
    table_state: &mut TableState,
    location: &str,
    error: &Option<String>,
    scroll_offset: u16,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header/Tabs
            Constraint::Min(0),    // Main content
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    // 1. Header/Tabs
    let titles = vec!["Inbox (1)", "Later (2)", "Archive (3)", "Feed (4)"];
    let current_tab = match location {
        "new" => 0,
        "later" => 1,
        "archive" => 2,
        "feed" => 3,
        _ => 0,
    };
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(" Readwise Reader "))
        .select(current_tab)
        .highlight_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);

    // 2. Main Content
    match view {
        ViewState::List => {
            let header_cells = ["", "Title", "Author", "Source", "Progress"]
                .iter()
                .map(|h| Cell::from(*h).style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
            let header = Row::new(header_cells)
                .style(Style::default().bg(Color::DarkGray))
                .height(1)
                .bottom_margin(1);

            let rows = articles.iter().map(|doc| {
                // Better source display logic
                let source_display = if let Some(sn) = &doc.site_name {
                    if sn != "Reader RSS" { sn.clone() } else { get_hostname(&doc.source_url) }
                } else if let Some(s) = &doc.source {
                    if s != "Reader RSS" { s.clone() } else { get_hostname(&doc.source_url) }
                } else {
                    get_hostname(&doc.source_url)
                };

                let unread_marker = if !doc.is_seen() { "●" } else { " " };
                let progress = if doc.reading_progress > 0.0 {
                    format!("{:.0}%", doc.reading_progress * 100.0)
                } else if doc.is_seen() {
                    "0%".to_string()
                } else {
                    "".to_string()
                };

                Row::new(vec![
                    Cell::from(unread_marker).style(Style::default().fg(Color::Yellow)),
                    Cell::from(doc.title.clone()),
                    Cell::from(doc.author.as_deref().unwrap_or("Unknown")),
                    Cell::from(source_display),
                    Cell::from(progress),
                ])
            });

            let table = Table::new(
                rows,
                [
                    Constraint::Length(1),
                    Constraint::Percentage(40),
                    Constraint::Percentage(20),
                    Constraint::Percentage(30),
                    Constraint::Length(8),
                ],
            )
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(format!(" {} ", location.to_uppercase())))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol(">> ");

            f.render_stateful_widget(table, chunks[1], table_state);
        }
        ViewState::Read { doc, content } => {
            let p = Paragraph::new(content.as_str())
                .block(Block::default().borders(Borders::ALL).title(format!(" {} ", doc.title)))
                .wrap(Wrap { trim: true })
                .scroll((scroll_offset, 0));
            f.render_widget(p, chunks[1]);

            // Add scrollbar
            let lines = content.lines().count();
            if lines > 0 {
                let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("↑"))
                    .end_symbol(Some("↓"));
                let mut scrollbar_state = ScrollbarState::new(lines).position(scroll_offset as usize);
                f.render_stateful_widget(
                    scrollbar,
                    chunks[1].inner(Margin { vertical: 1, horizontal: 0 }),
                    &mut scrollbar_state,
                );
            }
        }
    }

    // 3. Footer
    let help_text = match view {
        ViewState::List => " [j/k] Scroll | [Enter] Read | [m] Toggle Read | [a] Archive | [1-4] Tabs | [n/p] Page | [q] Quit ",
        ViewState::Read { .. } => " [j/k] Scroll | [m] Toggle Read | [a] Archive | [Esc/q] Back to list ",
    };
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(help_text, Style::default().add_modifier(Modifier::DIM)),
        if let Some(err) = error {
            Span::styled(format!(" ERROR: {}", err), Style::default().fg(Color::Red))
        } else {
            Span::raw("")
        },
    ]));
    f.render_widget(footer, chunks[2]);
}
