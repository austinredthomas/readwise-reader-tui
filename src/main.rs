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

use crate::api::{ReaderClient, Document};
use crate::ui::ViewState;

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

    async fn fetch_article_content(&mut self, doc: Document) {
        match self.client.list_documents(&self.location, Some(doc.id.clone()), true).await {
            Ok(res) => {
                if let Some(article) = res.results.into_iter().next() {
                    self.view = ViewState::Read(article);
                    self.scroll_offset = 0;
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
                                    app.fetch_article_content(doc_clone).await;
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
                        ViewState::Read(_) => match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                app.view = ViewState::List;
                                app.scroll_offset = 0;
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                app.scroll_offset = app.scroll_offset.saturating_add(1);
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                app.scroll_offset = app.scroll_offset.saturating_sub(1);
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
