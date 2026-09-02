use ratatui::widgets::ListState;
use std::collections::{HashSet, VecDeque};

/// Represents a file or folder from Google Drive
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    pub mime_type: String,
}

/// Represents background network actions that update the UI
pub enum Action {
    LoadFiles(Vec<DriveFile>),
    LoadTrash(Vec<DriveFile>),
    Error(String),
    Message(String),
    LoadQuota(u64, u64), // (used, limit)
    UploadComplete(String),
    ImagePreview(Vec<u8>),
    QueueUploads(Vec<UploadTask>),
    TokenRefreshed(crate::auth::Token),
    UpdateUploadProgress(String, u64, u64, f64), // local_path, uploaded, total, speed
    CompleteUpload(String),                      // local_path

    QueueDownloads(Vec<DriveFile>),
    UpdateDownloadProgress(String, u64, u64, f64), // id, downloaded, total, speed
    CompleteDownload(String),                      // id
}

#[derive(PartialEq)]
pub enum InputMode {
    Normal,
    UploadModal,
    DeleteConfirmModal,
    DownloadConfirmModal,
    TrashView,
    TrashDeleteConfirmModal,
    TrashDeleteAllConfirmModal,
    DownloadTrackerView,
    UploadTrackerView,
}

/// Main event enum for the TUI event loop
pub enum Event {
    Input(crossterm::event::KeyEvent),
    Action(Action),
    SuspendAndEdit(DriveFile),
    Resize(u16, u16),
    Tick,
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
    pub state: ratatui::widgets::ListState,
}

impl UploadManager {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            speed_history: VecDeque::from(vec![0; 100]),
            state: ratatui::widgets::ListState::default(),
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
    pub state: ratatui::widgets::ListState,
}

impl DownloadManager {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            speed_history: VecDeque::from(vec![0; 100]),
            state: ratatui::widgets::ListState::default(),
        }
    }
}

/// Core application state
pub struct App {
    pub files: Vec<DriveFile>,
    pub dl_manager: DownloadManager,
    pub active_dl_task: Option<tokio::task::JoinHandle<()>>,
    pub ul_manager: UploadManager,
    pub active_ul_task: Option<tokio::task::JoinHandle<()>>,
    pub selected_files: HashSet<String>,
    pub trashed_files: Vec<DriveFile>,
    pub state: ListState,
    pub trash_state: ListState,
    pub current_path: String,
    pub status: String,
    pub should_quit: bool,
    pub download_progress: Option<(u64, u64, f64)>,
    pub search_mode: bool,
    pub search_query: String,
    pub history: Vec<String>,
    pub path_names: Vec<String>,
    pub storage_quota: Option<(u64, u64)>,
    pub show_preview: bool,
    pub preview_image: Option<ratatui_image::protocol::StatefulProtocol>,
    pub preview_dims: Option<(u32, u32)>,
    pub picker: ratatui_image::picker::Picker,
    pub input_mode: InputMode,
    pub upload_target_id: String,
    pub upload_local_path: String,
    pub upload_input_idx: usize, // 0 = target, 1 = file path
    pub upload_progress: Option<(u64, u64, f64)>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn save_queues(&self) {
        if let Ok(config_dir) = crate::auth::get_config_dir() {
            let path = config_dir.join("queues.json");
            if self.dl_manager.queue.is_empty() && self.ul_manager.queue.is_empty() {
                let _ = std::fs::remove_file(&path);
                return;
            }
            #[derive(serde::Serialize)]
            struct Queues<'a> {
                downloads: &'a Vec<DownloadTask>,
                uploads: &'a Vec<UploadTask>,
            }
            let q = Queues {
                downloads: &self.dl_manager.queue,
                uploads: &self.ul_manager.queue,
            };
            if let Ok(json) = serde_json::to_string(&q) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    pub fn load_queues(&mut self) {
        if let Ok(config_dir) = crate::auth::get_config_dir() {
            let path = config_dir.join("queues.json");
            if let Ok(json) = std::fs::read_to_string(path) {
                #[derive(serde::Deserialize)]
                struct Queues {
                    downloads: Vec<DownloadTask>,
                    uploads: Vec<UploadTask>,
                }
                if let Ok(mut q) = serde_json::from_str::<Queues>(&json) {
                    for task in &mut q.downloads {
                        if task.status == DownloadStatus::Downloading {
                            task.status = DownloadStatus::Paused;
                        }
                    }
                    for task in &mut q.uploads {
                        if task.status == UploadStatus::Uploading {
                            task.status = UploadStatus::Paused;
                        }
                    }
                    self.dl_manager.queue = q.downloads;
                    self.ul_manager.queue = q.uploads;
                }
            }
        }
    }

    /// Initializes a new application state
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            files: Vec::new(),
            dl_manager: DownloadManager::new(),
            active_dl_task: None,
            ul_manager: UploadManager::new(),
            active_ul_task: None,
            trashed_files: Vec::new(),
            selected_files: HashSet::new(),
            state,
            trash_state: ListState::default(),
            current_path: "root".to_string(),
            status: "Loading...".to_string(),
            should_quit: false,
            download_progress: None,
            search_mode: false,
            search_query: String::new(),
            history: Vec::new(),
            path_names: vec!["root".to_string()],
            storage_quota: None,
            show_preview: false,
            preview_image: None,
            preview_dims: None,
            picker: ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
            input_mode: InputMode::Normal,
            upload_target_id: String::new(),
            upload_local_path: String::new(),
            upload_input_idx: 1, // Default focus on file path
            upload_progress: None,
        }
    }

    /// Selects the next file in the list
    pub fn next(&mut self) {
        if self.files.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.files.len() - 1 {
                    self.files.len() - 1
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Selects the previous file in the list
    pub fn previous(&mut self) {
        if self.files.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.state.select(Some(i));
    }

    /// Returns a reference to the currently selected file
    pub fn selected_file(&self) -> Option<&DriveFile> {
        self.state.selected().and_then(|i| self.files.get(i))
    }
}
