mod api;
mod config;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::{Duration, Instant};

use crate::api::{ReaderClient, Document, UpdateDocumentRequest};
use crate::ui::ViewState;

const FEED_REFRESH_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes

struct App {
    client: ReaderClient,
    location: String,
    articles: Vec<Document>,
    selected_index: usize,
    view: ViewState,
    current_cursor: Option<String>,
    next_page_cursor: Option<String>,
    prev_page_cursors: Vec<Option<String>>,
    error: Option<String>,
    scroll_offset: u16,
    last_feed_update: Instant,
}

impl App {
    async fn new(config: config::AppConfig) -> Self {
        let client = ReaderClient::new(config.token);
        Self {
            client,
            location: config.default_location,
            articles: Vec::new(),
            selected_index: 0,
            view: ViewState::List,
            current_cursor: None,
            next_page_cursor: None,
            prev_page_cursors: Vec::new(),
            error: None,
            scroll_offset: 0,
            last_feed_update: Instant::now(),
        }
    }

    async fn fetch_articles(&mut self, cursor: Option<String>, push_history: bool) {
        match self.client.list_documents(&self.location, cursor.clone(), false).await {
            Ok(res) => {
                if push_history {
                    self.prev_page_cursors.push(self.current_cursor.clone());
                }
                self.articles = res.results;
                self.next_page_cursor = res.next_page_cursor;
                self.current_cursor = cursor;
                self.selected_index = 0;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    async fn fetch_article_content(&mut self, doc: Document, width: u16) {
        match self.client.list_documents(&self.location, Some(doc.id.clone()), true).await {
            Ok(res) => {
                if let Some(article) = res.results.into_iter().next() {
                    let content = if let Some(html) = &article.html_content {
                        match html2text::from_read(html.as_bytes(), width as usize - 4) {
                            Ok(text) => text,
                            Err(e) => format!("Error parsing content: {}", e),
                        }
                    } else {
                        "No content available.".to_string()
                    };
                    self.view = ViewState::Read { doc: article, content };
                    self.scroll_offset = 0;
                }
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    async fn toggle_seen(&mut self, doc_id: String, current_seen: bool) {
        let new_seen = !current_seen;
        match self.client.update_document(&doc_id, UpdateDocumentRequest {
            seen: Some(new_seen),
            location: None,
        }).await {
            Ok(_) => {
                // Update local state
                if let ViewState::Read { ref mut doc, .. } = self.view {
                    if doc.id == doc_id {
                        doc.seen = new_seen;
                    }
                }
                for doc in &mut self.articles {
                    if doc.id == doc_id {
                        doc.seen = new_seen;
                    }
                }
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    async fn archive_document(&mut self, doc_id: String) {
        match self.client.update_document(&doc_id, UpdateDocumentRequest {
            seen: Some(true),
            location: Some("archive".to_string()),
        }).await {
            Ok(_) => {
                // Remove from local list if present
                self.articles.retain(|d| d.id != doc_id);
                if self.selected_index >= self.articles.len() && !self.articles.is_empty() {
                    self.selected_index = self.articles.len() - 1;
                }
                let mut go_back = false;
                if let ViewState::Read { doc, .. } = &self.view {
                    if doc.id == doc_id {
                        go_back = true;
                    }
                }
                if go_back {
                    self.view = ViewState::List;
                }
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::load_config()?;
    let mut app = App::new(config).await;
    app.fetch_articles(None, false).await;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut should_quit = false;
    while !should_quit {
        // Auto-update feed if viewing it
        if app.location == "feed" && app.last_feed_update.elapsed() >= FEED_REFRESH_INTERVAL {
            if let ViewState::List = app.view {
                app.fetch_articles(None, false).await;
                app.last_feed_update = Instant::now();
            }
        }

        terminal.draw(|f| {
            ui::draw(
                f,
                &app.view,
                &app.articles,
                app.selected_index,
                &app.location,
                &app.error,
                app.scroll_offset,
            )
        })?;

        if event::poll(std::time::Duration::from_millis(10))? {
            // Process all pending events to avoid input lag/queueing
            while event::poll(std::time::Duration::from_millis(0))? {
                if let Event::Key(key) = event::read()? {
                    // Only handle KeyPress events (not release)
                    if key.kind != event::KeyEventKind::Press {
                        continue;
                    }

                    match &app.view {
                        ViewState::List => match key.code {
                            KeyCode::Char('q') => {
                                should_quit = true;
                                break;
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                if !app.articles.is_empty() && app.selected_index < app.articles.len() - 1 {
                                    app.selected_index += 1;
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                if app.selected_index > 0 {
                                    app.selected_index -= 1;
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(doc) = app.articles.get(app.selected_index) {
                                    let doc_clone = doc.clone();
                                    let width = terminal.size()?.width;
                                    app.fetch_article_content(doc_clone, width).await;
                                }
                            }
                            KeyCode::Char('m') => {
                                if let Some(doc) = app.articles.get(app.selected_index) {
                                    let id = doc.id.clone();
                                    let seen = doc.seen;
                                    app.toggle_seen(id, seen).await;
                                }
                            }
                            KeyCode::Char('a') => {
                                if let Some(doc) = app.articles.get(app.selected_index) {
                                    let id = doc.id.clone();
                                    app.archive_document(id).await;
                                }
                            }
                            KeyCode::Char('1') => {
                                app.location = "new".to_string();
                                app.prev_page_cursors.clear();
                                app.fetch_articles(None, false).await;
                            }
                            KeyCode::Char('2') => {
                                app.location = "later".to_string();
                                app.prev_page_cursors.clear();
                                app.fetch_articles(None, false).await;
                            }
                            KeyCode::Char('3') => {
                                app.location = "archive".to_string();
                                app.prev_page_cursors.clear();
                                app.fetch_articles(None, false).await;
                            }
                            KeyCode::Char('4') => {
                                app.location = "feed".to_string();
                                app.prev_page_cursors.clear();
                                app.fetch_articles(None, false).await;
                                app.last_feed_update = Instant::now();
                            }

                            KeyCode::Char('n') => {
                                if let Some(cursor) = app.next_page_cursor.clone() {
                                    app.fetch_articles(Some(cursor), true).await;
                                }
                            }
                            KeyCode::Char('p') => {
                                if let Some(prev_cursor) = app.prev_page_cursors.pop() {
                                    app.fetch_articles(prev_cursor, false).await;
                                }
                            }
                            _ => {}
                        },
                        ViewState::Read { doc, content } => match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                app.view = ViewState::List;
                                app.scroll_offset = 0;
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                let lines = content.lines().count() as u16;
                                let height = terminal.size()?.height.saturating_sub(4); // -3 for header, -1 for footer
                                if app.scroll_offset + height < lines {
                                    app.scroll_offset = app.scroll_offset.saturating_add(1);
                                }

                                // Auto-mark as read if near bottom
                                if !doc.seen && app.scroll_offset + height >= lines.saturating_sub(2) {
                                    let id = doc.id.clone();
                                    app.toggle_seen(id, false).await;
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                app.scroll_offset = app.scroll_offset.saturating_sub(1);
                            }
                            KeyCode::Char('m') => {
                                let id = doc.id.clone();
                                let seen = doc.seen;
                                app.toggle_seen(id, seen).await;
                            }
                            KeyCode::Char('a') => {
                                let id = doc.id.clone();
                                app.archive_document(id).await;
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
