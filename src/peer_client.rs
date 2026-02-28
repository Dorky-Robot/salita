use crate::error::{AppError, AppResult};
use crate::files::{FileEntry, FileInfo};

/// HTTP client for calling peer node APIs
pub struct PeerClient {
    client: reqwest::Client,
}

impl PeerClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    fn base_url(endpoint: &str, port: u16) -> String {
        format!("http://{}:{}", endpoint, port)
    }

    pub async fn list_files(
        &self,
        endpoint: &str,
        port: u16,
        dir: &str,
        path: &str,
    ) -> AppResult<Vec<FileEntry>> {
        let url = format!(
            "{}/api/v1/files?dir={}&path={}",
            Self::base_url(endpoint, port),
            dir,
            path
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Peer request failed: {}", e)))?;

        resp.json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse peer response: {}", e)))
    }

    pub async fn search_files(
        &self,
        endpoint: &str,
        port: u16,
        pattern: &str,
        dir: Option<&str>,
    ) -> AppResult<Vec<FileEntry>> {
        let mut url = format!(
            "{}/api/v1/files/search?pattern={}",
            Self::base_url(endpoint, port),
            pattern
        );
        if let Some(d) = dir {
            url.push_str(&format!("&dir={}", d));
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Peer request failed: {}", e)))?;

        resp.json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse peer response: {}", e)))
    }

    pub async fn read_file(
        &self,
        endpoint: &str,
        port: u16,
        dir: &str,
        path: &str,
    ) -> AppResult<String> {
        let url = format!(
            "{}/api/v1/files/read?dir={}&path={}",
            Self::base_url(endpoint, port),
            dir,
            path
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Peer request failed: {}", e)))?;

        resp.text()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read peer response: {}", e)))
    }

    pub async fn file_info(
        &self,
        endpoint: &str,
        port: u16,
        dir: &str,
        path: &str,
    ) -> AppResult<FileInfo> {
        let url = format!(
            "{}/api/v1/files/info?dir={}&path={}",
            Self::base_url(endpoint, port),
            dir,
            path
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Peer request failed: {}", e)))?;

        resp.json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse peer response: {}", e)))
    }
}
