use anyhow::Result;
use reqwest::{header, Client as HttpClient};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub source: Option<String>,
    pub site_name: Option<String>,
    pub source_url: String,
    pub location: Option<String>,
    pub category: Option<String>,
    pub html_content: Option<String>,
    pub created_at: String,
    pub first_opened_at: Option<String>,
    pub last_opened_at: Option<String>,
    pub reading_progress: f32,
    pub published_date: Option<String>,
    pub summary: Option<String>,
}

impl Document {
    pub fn is_seen(&self) -> bool {
        self.first_opened_at.is_some()
    }
}

#[derive(Debug, Serialize)]
pub struct UpdateDocumentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListResponse {
    pub results: Vec<Document>,
    #[serde(rename = "nextPageCursor")]
    pub next_page_cursor: Option<String>,
}

pub struct ReaderClient {
    http: HttpClient,
}

impl ReaderClient {
    pub fn new(token: String) -> Self {
        let mut headers = header::HeaderMap::new();
        let mut auth_value = header::HeaderValue::from_str(&format!("Token {}", token)).expect("Invalid token");
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);

        let http = HttpClient::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to build HTTP client");

        Self { http }
    }

    pub async fn list_documents(
        &self,
        location: &str,
        page_cursor: Option<String>,
        document_id: Option<String>,
        with_html: bool,
    ) -> Result<ListResponse> {
        let mut query = vec![("location", location.to_string())];
        if let Some(cursor) = page_cursor {
            query.push(("pageCursor", cursor));
        }
        if let Some(id) = document_id {
            query.push(("id", id));
        }
        if with_html {
            query.push(("withHtmlContent", "true".to_string()));
        }

        let res = self
            .http
            .get("https://readwise.io/api/v3/list/")
            .query(&query)
            .send()
            .await?
            .json::<ListResponse>()
            .await?;

        Ok(res)
    }

    pub async fn update_document(
        &self,
        id: &str,
        update: UpdateDocumentRequest,
    ) -> Result<()> {
        self.http
            .patch(&format!("https://readwise.io/api/v3/update/{}/", id))
            .json(&update)
            .send()
            .await?;

        Ok(())
    }
}
