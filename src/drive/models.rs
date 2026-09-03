use ratatui::widgets::ListState;
use std::collections::VecDeque;

/// Represents a file or folder from Google Drive
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
}

#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum UploadStatus {
    Pending,
    Uploading,
    Paused,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UploadTask {
    pub local_path: String,
    pub name: String,
    pub target_parent_id: String,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub status: UploadStatus,
}

pub struct UploadManager {
    pub queue: Vec<UploadTask>,
    pub speed_history: VecDeque<u64>,
    pub state: ListState,
}

impl UploadManager {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            speed_history: VecDeque::from(vec![0; 100]),
            state: ListState::default(),
        }
    }
}

#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Paused,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DownloadTask {
    pub file: DriveFile,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub status: DownloadStatus,
}

pub struct DownloadManager {
    pub queue: Vec<DownloadTask>,
    pub speed_history: VecDeque<u64>,
    pub state: ListState,
}

impl DownloadManager {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            speed_history: VecDeque::from(vec![0; 100]),
            state: ListState::default(),
        }
    }
}
