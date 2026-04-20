mod api;
mod config;
mod ui;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, widgets::TableState};
use std::io;
use std::time::{Duration, Instant};

use crate::api::{Document, ReaderClient, UpdateDocumentRequest};
use crate::ui::ViewState;

const FEED_REFRESH_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes

struct App {
    client: ReaderClient,
    location: String,
    articles: Vec<Document>,
    table_state: TableState,
    view: ViewState,
    current_cursor: Option<String>,
    next_page_cursor: Option<String>,
    prev_page_cursors: Vec<Option<String>>,
    error: Option<String>,
    scroll_offset: u16,
    last_feed_update: Instant,
    pending_progress: Option<(String, String, f32)>, // (id, url, progress)
    last_scroll_event: Instant,
}

impl App {
    async fn new(config: config::AppConfig) -> Self {
        let client = ReaderClient::new(config.token);
        Self {
            client,
            location: config.default_location,
            articles: Vec::new(),
            table_state: TableState::default(),
            view: ViewState::List,
            current_cursor: None,
            next_page_cursor: None,
            prev_page_cursors: Vec::new(),
            error: None,
            scroll_offset: 0,
            last_feed_update: Instant::now(),
            pending_progress: None,
            last_scroll_event: Instant::now(),
        }
    }

    async fn fetch_articles(&mut self, cursor: Option<String>, push_history: bool) {
        match self
            .client
            .list_documents(Some(&self.location), cursor.clone(), None, false)
            .await
        {
            Ok(res) => {
                if push_history {
                    self.prev_page_cursors.push(self.current_cursor.clone());
                }
                let mut results = res.results;
                for doc in &mut results {
                    // Always try to extract progress from tags first
                    if let Some(tags) = &doc.tags {
                        for tag in tags.keys() {
                            if tag.starts_with("progress:") {
                                if let Ok(p) = tag["progress:".len()..].parse::<f32>() {
                                    doc.reading_progress = p / 100.0;
                                    break;
                                }
                            }
                        }
                    }
                }
                self.articles = results;
                self.next_page_cursor = res.next_page_cursor;
                self.current_cursor = cursor;
                self.table_state.select(Some(0));
                self.error = None;
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    async fn fetch_article_content(&mut self, doc: Document, width: u16, height: u16) {
        match self
            .client
            .list_documents(None, None, Some(doc.id.clone()), true)
            .await
        {
            Ok(res) => {
                if let Some(mut article) = res.results.into_iter().next() {
                    // Always try to extract progress from tags first
                    if let Some(tags) = &article.tags {
                        for tag in tags.keys() {
                            if tag.starts_with("progress:") {
                                if let Ok(p) = tag["progress:".len()..].parse::<f32>() {
                                    article.reading_progress = p / 100.0;
                                    break;
                                }
                            }
                        }
                    }

                    let content = if let Some(html) = &article.html_content {
                        match html2text::from_read(html.as_bytes(), width as usize - 4) {
                            Ok(text) => text,
                            Err(e) => format!("Error parsing content: {}", e),
                        }
                    } else {
                        "No content available.".to_string()
                    };

                    let lines = content.lines().count() as f32;
                    let v_height = height.saturating_sub(4) as f32;
                    let initial_scroll = if article.reading_progress > 0.0 && lines > v_height {
                        (article.reading_progress * lines - v_height).max(0.0) as u16
                    } else {
                        0
                    };

                    // Mark as seen locally
                    let mut article_clone = article.clone();
                    if article_clone.first_opened_at.is_none() {
                        article_clone.first_opened_at = Some("local".to_string());
                        let id = article_clone.id.clone();
                        let url = article_clone.source_url.clone();
                        for d in &mut self.articles {
                            if d.id == id || (d.source_url == url && !url.is_empty()) {
                                d.first_opened_at = Some("local".to_string());
                            }
                        }
                    }

                    self.view = ViewState::Read {
                        doc: article_clone,
                        content,
                    };
                    self.scroll_offset = initial_scroll;
                }
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    async fn toggle_seen(&mut self, doc_id: String, source_url: String, current_seen: bool) {
        let new_seen = !current_seen;
        let mut tags_to_update = None;
        if new_seen {
            // Get current tag names to preserve them, but set progress to 100
            let mut current_tag_names = Vec::new();
            if let ViewState::Read { ref mut doc, .. } = self.view {
                if doc.id == doc_id || doc.source_url == source_url {
                    current_tag_names = doc
                        .tags
                        .as_ref()
                        .map(|t| t.keys().cloned().collect())
                        .unwrap_or_default();
                }
            }
            if current_tag_names.is_empty() {
                for doc in &self.articles {
                    if doc.id == doc_id || (doc.source_url == source_url && !source_url.is_empty())
                    {
                        current_tag_names = doc
                            .tags
                            .as_ref()
                            .map(|t| t.keys().cloned().collect())
                            .unwrap_or_default();
                        break;
                    }
                }
            }
            let mut new_tags: Vec<String> = current_tag_names
                .into_iter()
                .filter(|t| !t.starts_with("progress:"))
                .collect();
            new_tags.push("progress:100".to_string());
            tags_to_update = Some(new_tags);
        }

        match self
            .client
            .update_document(
                &doc_id,
                UpdateDocumentRequest {
                    seen: Some(new_seen),
                    location: None,
                    reading_progress: if new_seen { Some(1.0) } else { None },
                    tags: tags_to_update.clone(),
                },
            )
            .await
        {
            Ok(_) => {
                // Update local state
                if let ViewState::Read { ref mut doc, .. } = self.view {
                    if doc.id == doc_id || doc.source_url == source_url {
                        doc.first_opened_at = if new_seen {
                            Some("local".to_string())
                        } else {
                            None
                        };
                        if new_seen {
                            doc.reading_progress = 1.0;
                        }
                    }
                }
                for doc in &mut self.articles {
                    if doc.id == doc_id || (doc.source_url == source_url && !source_url.is_empty())
                    {
                        doc.first_opened_at = if new_seen {
                            Some("local".to_string())
                        } else {
                            None
                        };
                        if new_seen {
                            doc.reading_progress = 1.0;
                        }
                    }
                }
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    async fn archive_document(&mut self, doc_id: String, source_url: String) {
        let mut current_tag_names = Vec::new();
        if let ViewState::Read { ref mut doc, .. } = self.view {
            if doc.id == doc_id || doc.source_url == source_url {
                current_tag_names = doc
                    .tags
                    .as_ref()
                    .map(|t| t.keys().cloned().collect())
                    .unwrap_or_default();
            }
        }
        if current_tag_names.is_empty() {
            for doc in &self.articles {
                if doc.id == doc_id || (doc.source_url == source_url && !source_url.is_empty()) {
                    current_tag_names = doc
                        .tags
                        .as_ref()
                        .map(|t| t.keys().cloned().collect())
                        .unwrap_or_default();
                    break;
                }
            }
        }
        let mut new_tags: Vec<String> = current_tag_names
            .into_iter()
            .filter(|t| !t.starts_with("progress:"))
            .collect();
        new_tags.push("progress:100".to_string());

        match self
            .client
            .update_document(
                &doc_id,
                UpdateDocumentRequest {
                    seen: Some(true),
                    location: Some("archive".to_string()),
                    reading_progress: Some(1.0),
                    tags: Some(new_tags),
                },
            )
            .await
        {
            Ok(_) => {
                // Remove from local list if present
                self.articles.retain(|d| {
                    d.id != doc_id && (d.source_url != source_url || source_url.is_empty())
                });

                // Adjust selection if it went out of bounds
                let len = self.articles.len();
                if let Some(selected) = self.table_state.selected() {
                    if selected >= len && len > 0 {
                        self.table_state.select(Some(len - 1));
                    } else if len == 0 {
                        self.table_state.select(None);
                    }
                }

                let mut go_back = false;
                if let ViewState::Read { doc, .. } = &self.view {
                    if doc.id == doc_id || doc.source_url == source_url {
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

    async fn move_document(&mut self, doc_id: String, source_url: String, target_location: &str) {
        match self
            .client
            .update_document(
                &doc_id,
                UpdateDocumentRequest {
                    location: Some(target_location.to_string()),
                    seen: None,
                    reading_progress: None,
                    tags: None,
                },
            )
            .await
        {
            Ok(_) => {
                // If the current view is not the target location, remove it from the local list
                if self.location != target_location {
                    self.articles.retain(|d| {
                        d.id != doc_id && (d.source_url != source_url || source_url.is_empty())
                    });

                    // Adjust selection
                    let len = self.articles.len();
                    if let Some(selected) = self.table_state.selected() {
                        if selected >= len && len > 0 {
                            self.table_state.select(Some(len - 1));
                        } else if len == 0 {
                            self.table_state.select(None);
                        }
                    }

                    let mut go_back = false;
                    if let ViewState::Read { doc, .. } = &self.view {
                        if doc.id == doc_id || doc.source_url == source_url {
                            go_back = true;
                        }
                    }
                    if go_back {
                        self.view = ViewState::List;
                    }
                }
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }
    }

    async fn update_reading_progress(&mut self, doc_id: String, source_url: String, progress: f32) {
        // Update local state
        let mut found = false;
        let mut current_tag_names = Vec::new();
        if let ViewState::Read { ref mut doc, .. } = self.view {
            if doc.id == doc_id || doc.source_url == source_url {
                doc.reading_progress = progress;
                current_tag_names = doc
                    .tags
                    .as_ref()
                    .map(|t| t.keys().cloned().collect())
                    .unwrap_or_default();
                found = true;
            }
        }
        for doc in &mut self.articles {
            if doc.id == doc_id || (doc.source_url == source_url && !source_url.is_empty()) {
                doc.reading_progress = progress;
                if current_tag_names.is_empty() {
                    current_tag_names = doc
                        .tags
                        .as_ref()
                        .map(|t| t.keys().cloned().collect())
                        .unwrap_or_default();
                }
                found = true;
            }
        }

        if found {
            // Update tags with new progress
            let progress_tag = format!("progress:{}", (progress * 100.0).round() as i32);
            let mut new_tags: Vec<String> = current_tag_names
                .into_iter()
                .filter(|t| !t.starts_with("progress:"))
                .collect();
            new_tags.push(progress_tag);

            if let Err(e) = self
                .client
                .update_document(
                    &doc_id,
                    UpdateDocumentRequest {
                        reading_progress: Some(progress),
                        location: None,
                        seen: None,
                        tags: Some(new_tags.clone()),
                    },
                )
                .await
            {
                self.error = Some(format!("Failed to update progress: {}", e));
            } else {
                // Update local tags too (we don't have the full Value but we can clear/repopulate the keys if we want,
                // but local state just needs the progress field updated which we already did)
                // For simplicity, we just leave the local tags as they were, but progress is updated.
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

        // Handle debounced progress updates
        if let Some((id, url, progress)) = app.pending_progress.clone() {
            if app.last_scroll_event.elapsed() >= Duration::from_secs(2) {
                app.update_reading_progress(id, url, progress).await;
                app.pending_progress = None;
            }
        }

        terminal.draw(|f| {
            ui::draw(
                f,
                &app.view,
                &app.articles,
                &mut app.table_state,
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

                    match &mut app.view {
                        ViewState::List => match key.code {
                            KeyCode::Char('q') => {
                                should_quit = true;
                                break;
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                if !app.articles.is_empty() {
                                    let i = match app.table_state.selected() {
                                        Some(i) => {
                                            if i >= app.articles.len() - 1 {
                                                0
                                            } else {
                                                i + 1
                                            }
                                        }
                                        None => 0,
                                    };
                                    app.table_state.select(Some(i));
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                if !app.articles.is_empty() {
                                    let i = match app.table_state.selected() {
                                        Some(i) => {
                                            if i == 0 {
                                                app.articles.len() - 1
                                            } else {
                                                i - 1
                                            }
                                        }
                                        None => 0,
                                    };
                                    app.table_state.select(Some(i));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(i) = app.table_state.selected() {
                                    if let Some(doc) = app.articles.get(i) {
                                        let doc_clone = doc.clone();
                                        let size = terminal.size()?;
                                        app.fetch_article_content(
                                            doc_clone,
                                            size.width,
                                            size.height,
                                        )
                                        .await;
                                    }
                                }
                            }
                            KeyCode::Char('m') => {
                                if let Some(i) = app.table_state.selected() {
                                    if let Some(doc) = app.articles.get(i) {
                                        let id = doc.id.clone();
                                        let url = doc.source_url.clone();
                                        let seen = doc.is_seen();
                                        app.toggle_seen(id, url, seen).await;
                                    }
                                }
                            }
                            KeyCode::Char('a') => {
                                if let Some(i) = app.table_state.selected() {
                                    if let Some(doc) = app.articles.get(i) {
                                        let id = doc.id.clone();
                                        let url = doc.source_url.clone();
                                        app.archive_document(id, url).await;
                                    }
                                }
                            }
                            KeyCode::Char('i') => {
                                if let Some(i) = app.table_state.selected() {
                                    if let Some(doc) = app.articles.get(i) {
                                        let id = doc.id.clone();
                                        let url = doc.source_url.clone();
                                        app.move_document(id, url, "new").await;
                                    }
                                }
                            }
                            KeyCode::Char('l') => {
                                if let Some(i) = app.table_state.selected() {
                                    if let Some(doc) = app.articles.get(i) {
                                        let id = doc.id.clone();
                                        let url = doc.source_url.clone();
                                        app.move_document(id, url, "later").await;
                                    }
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
                                // Flush pending progress immediately
                                if let Some((id, url, progress)) = app.pending_progress.take() {
                                    app.update_reading_progress(id, url, progress).await;
                                } else {
                                    let lines = content.lines().count() as f32;
                                    let height = terminal.size()?.height.saturating_sub(4) as f32;
                                    let progress = if lines > height {
                                        (app.scroll_offset as f32 + height) / lines
                                    } else {
                                        1.0
                                    };
                                    let doc_id = doc.id.clone();
                                    let source_url = doc.source_url.clone();
                                    app.update_reading_progress(
                                        doc_id,
                                        source_url,
                                        progress.min(1.0),
                                    )
                                    .await;
                                }

                                app.view = ViewState::List;
                                app.scroll_offset = 0;
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                let lines_count = content.lines().count();
                                let lines = lines_count as u16;
                                let height = terminal.size()?.height.saturating_sub(4); // -3 for header, -1 for footer
                                if app.scroll_offset + height < lines {
                                    app.scroll_offset = app.scroll_offset.saturating_add(1);
                                }

                                // Update local progress
                                let progress = if lines_count as f32 > height as f32 {
                                    (app.scroll_offset as f32 + height as f32) / lines_count as f32
                                } else {
                                    1.0
                                };
                                let p = progress.min(1.0);
                                doc.reading_progress = p;
                                let doc_id = doc.id.clone();
                                let source_url = doc.source_url.clone();
                                for d in &mut app.articles {
                                    if d.id == doc_id
                                        || (d.source_url == source_url && !source_url.is_empty())
                                    {
                                        d.reading_progress = p;
                                    }
                                }

                                // Set pending update
                                app.pending_progress = Some((doc_id, source_url, p));
                                app.last_scroll_event = Instant::now();

                                // Auto-mark as read if near bottom
                                if !doc.is_seen()
                                    && app.scroll_offset + height >= lines.saturating_sub(2)
                                {
                                    let id = doc.id.clone();
                                    let url = doc.source_url.clone();
                                    app.toggle_seen(id, url, false).await;
                                }
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                app.scroll_offset = app.scroll_offset.saturating_sub(1);

                                let lines_count = content.lines().count();
                                let height = terminal.size()?.height.saturating_sub(4);
                                let progress = if lines_count as f32 > height as f32 {
                                    (app.scroll_offset as f32 + height as f32) / lines_count as f32
                                } else {
                                    1.0
                                };
                                let p = progress.min(1.0);
                                doc.reading_progress = p;
                                let doc_id = doc.id.clone();
                                let source_url = doc.source_url.clone();
                                for d in &mut app.articles {
                                    if d.id == doc_id
                                        || (d.source_url == source_url && !source_url.is_empty())
                                    {
                                        d.reading_progress = p;
                                    }
                                }

                                // Set pending update
                                app.pending_progress = Some((doc_id, source_url, p));
                                app.last_scroll_event = Instant::now();
                            }
                            KeyCode::Char('m') => {
                                let id = doc.id.clone();
                                let url = doc.source_url.clone();
                                let seen = doc.is_seen();
                                app.toggle_seen(id, url, seen).await;
                            }
                            KeyCode::Char('a') => {
                                let id = doc.id.clone();
                                let url = doc.source_url.clone();
                                app.archive_document(id, url).await;
                            }
                            KeyCode::Char('i') => {
                                let id = doc.id.clone();
                                let url = doc.source_url.clone();
                                app.move_document(id, url, "new").await;
                            }
                            KeyCode::Char('l') => {
                                let id = doc.id.clone();
                                let url = doc.source_url.clone();
                                app.move_document(id, url, "later").await;
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
