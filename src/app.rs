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
    Error(String),
    DownloadProgress(u64, u64, f64), // (downloaded, total, speed)
    DownloadComplete(String),
    Message(String),
    LoadQuota(u64, u64), // (used, limit)
    ImagePreview(Vec<u8>), // Fetched image bytes
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
    pub state: ListState,
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
    pub picker: ratatui_image::picker::Picker,
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
            state,
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
            picker: ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
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
