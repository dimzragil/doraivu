use ratatui::widgets::ListState;
use serde::Deserialize;

/// Represents a file or folder from Google Drive
#[derive(Clone, Debug, Deserialize)]
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
    DownloadProgress(u64, u64, f64), // (downloaded, total, speed)
    DownloadComplete(String),
    Message(String),
    LoadQuota(u64, u64),           // (used, limit)
    UploadProgress(u64, u64, f64), // (uploaded, total, speed)
    UploadComplete(String),
    ImagePreview(Vec<u8>), // Fetched image bytes
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
}

/// Main event enum for the TUI event loop
pub enum Event {
    Input(crossterm::event::KeyEvent),
    Action(Action),
    SuspendAndEdit(DriveFile),
}

/// Core application state
pub struct App {
    pub files: Vec<DriveFile>,
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
    /// Initializes a new application state
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            files: Vec::new(),
            trashed_files: Vec::new(),
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
