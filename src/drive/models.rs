use ratatui::widgets::ListState;
use std::collections::{HashMap, VecDeque};

/// Represents a file or folder from Google Drive
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    #[serde(default)]
    pub app_properties: Option<HashMap<String, String>>,
}

impl DriveFile {
    /// Returns the virtual root entries: "My Drive" and "Shared with me".
    pub fn virtual_root_items() -> Vec<Self> {
        vec![
            DriveFile {
                id: "root".to_string(),
                name: "My Drive".to_string(),
                mime_type: "application/vnd.google-apps.folder".to_string(),
                app_properties: None,
            },
            DriveFile {
                id: "shared_with_me".to_string(),
                name: "Shared with me".to_string(),
                mime_type: "application/vnd.google-apps.folder".to_string(),
                app_properties: None,
            },
        ]
    }
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

impl Default for UploadManager {
    fn default() -> Self {
        Self::new()
    }
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

impl Default for DownloadManager {
    fn default() -> Self {
        Self::new()
    }
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
