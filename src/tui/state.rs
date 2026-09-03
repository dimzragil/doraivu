use crate::drive::models::{
    DownloadManager, DownloadStatus, DownloadTask, DriveFile, UploadManager, UploadStatus,
    UploadTask,
};
use ratatui::widgets::ListState;
use std::collections::HashSet;

/// Preview state for the split-pane preview panel
pub enum PreviewState {
    None,
    Loading,
    Image(ratatui_image::protocol::StatefulProtocol),
    Metadata {
        name: String,
        size: Option<u64>,
        created: String,
        modified: String,
    },
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
    PreviewMetadataLoaded(String, Option<u64>, String, String),
    QueueUploads(Vec<UploadTask>),
    TokenRefreshed(crate::drive::auth::Token),
    TokenRefreshFailed,
    UpdateUploadProgress(String, u64, u64, f64), // local_path, uploaded, total, speed
    CompleteUpload(String),                      // local_path

    QueueDownloads(Vec<DriveFile>),
    UpdateDownloadProgress(String, u64, u64, f64), // id, downloaded, total, speed
    CompleteDownload(String),                      // id
    UpdateResumeTime(String, String),              // (id, resume_time)
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

#[derive(PartialEq, Clone, Copy)]
pub enum PreviewMode {
    Hidden,
    Default,
    ForceMetadata,
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
    pub preview_mode: PreviewMode,
    pub preview_state: PreviewState,
    pub preview_dims: Option<(u32, u32)>,
    pub picker: ratatui_image::picker::Picker,
    pub input_mode: InputMode,
    pub upload_target_id: String,
    pub upload_local_path: String,
    pub upload_input_idx: usize,
    pub upload_progress: Option<(u64, u64, f64)>,
    pub theme_color: ratatui::style::Color,
    pub token_refreshed_at: std::time::Instant,
    pub is_refreshing_token: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn save_queues(&self) {
        if let Ok(config_dir) = crate::drive::auth::get_config_dir() {
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
        if let Ok(config_dir) = crate::drive::auth::get_config_dir() {
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
            current_path: "virtual_root".to_string(),
            status: "Loading...".to_string(),
            should_quit: false,
            download_progress: None,
            search_mode: false,
            search_query: String::new(),
            history: Vec::new(),
            path_names: vec!["virtual_root".to_string()],
            storage_quota: None,
            preview_mode: PreviewMode::Hidden,
            preview_state: PreviewState::None,
            preview_dims: None,
            picker: ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
            input_mode: InputMode::Normal,
            upload_target_id: String::new(),
            upload_local_path: String::new(),
            upload_input_idx: 1,
            upload_progress: None,
            theme_color: ratatui::style::Color::Cyan,
            token_refreshed_at: std::time::Instant::now(),
            is_refreshing_token: false,
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
