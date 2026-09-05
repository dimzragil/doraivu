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
    RenameSuccess,
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
    SetDownloadReconnecting(String),               // id
    SetUploadReconnecting(String),                 // local_path
    RefreshFolder(String),                         // target_folder_id
    ClearClipboard,
}

#[derive(PartialEq, Debug)]
pub enum InputMode {
    Normal,
    RenameModal,
    NewFolderModal,
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

/// State related to downloads and the download queue
#[derive(Default)]
pub struct DownloadState {
    pub manager: DownloadManager,
    pub active_task: Option<tokio::task::JoinHandle<()>>,
    pub progress: Option<(u64, u64, f64)>,
}

/// State related to uploads and the upload queue/modal
pub struct UploadState {
    pub manager: UploadManager,
    pub active_task: Option<tokio::task::JoinHandle<()>>,
    pub progress: Option<(u64, u64, f64)>,
    pub target_id: String,
    pub local_path: String,
    pub input_idx: usize,
}

impl Default for UploadState {
    fn default() -> Self {
        Self {
            manager: UploadManager::new(),
            active_task: None,
            progress: None,
            target_id: String::new(),
            local_path: String::new(),
            input_idx: 1,
        }
    }
}

/// State for the file rename modal
#[derive(Default)]
pub struct RenameState {
    pub buffer: String,
    pub target_id: String,
}

/// State for in-list file search
#[derive(Default)]
pub struct SearchState {
    pub active: bool,
    pub query: String,
}

/// Context and state for the split preview pane
pub struct PreviewContext {
    pub mode: PreviewMode,
    pub state: PreviewState,
    pub dims: Option<(u32, u32)>,
    pub picker: ratatui_image::picker::Picker,
}

impl Default for PreviewContext {
    fn default() -> Self {
        Self {
            mode: PreviewMode::Hidden,
            state: PreviewState::None,
            dims: None,
            picker: ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
        }
    }
}

/// State for directory navigation and history
pub struct NavigationState {
    pub current_path: String,
    pub history: Vec<String>,
    pub path_names: Vec<String>,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            current_path: "virtual_root".to_string(),
            history: Vec::new(),
            path_names: vec!["virtual_root".to_string()],
        }
    }
}

/// State for trash bin items and selection
#[derive(Default)]
pub struct TrashState {
    pub files: Vec<DriveFile>,
    pub state: ListState,
}

/// Action type for virtual clipboard
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardAction {
    Copy,
    Move,
}

/// Virtual clipboard holding selected file IDs and action
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clipboard {
    pub action: ClipboardAction,
    pub file_ids: Vec<String>,
    pub source_parent_id: String,
}

/// Core application state
pub struct App {
    pub files: Vec<DriveFile>,
    pub selected_files: HashSet<String>,
    pub state: ListState,
    pub status: String,
    pub should_quit: bool,
    pub storage_quota: Option<(u64, u64)>,
    pub theme_color: ratatui::style::Color,
    pub input_mode: InputMode,
    pub token_refreshed_at: std::time::Instant,
    pub is_refreshing_token: bool,
    pub clipboard: Option<Clipboard>,
    pub new_folder_buffer: String,

    // Grouped sub-states
    pub download: DownloadState,
    pub upload: UploadState,
    pub rename: RenameState,
    pub search: SearchState,
    pub preview: PreviewContext,
    pub nav: NavigationState,
    pub trash: TrashState,
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
            if self.download.manager.queue.is_empty() && self.upload.manager.queue.is_empty() {
                let _ = std::fs::remove_file(&path);
                return;
            }
            #[derive(serde::Serialize)]
            struct Queues<'a> {
                downloads: &'a Vec<DownloadTask>,
                uploads: &'a Vec<UploadTask>,
            }
            let q = Queues {
                downloads: &self.download.manager.queue,
                uploads: &self.upload.manager.queue,
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
                        if task.status == DownloadStatus::Downloading
                            || task.status == DownloadStatus::Reconnecting
                        {
                            task.status = DownloadStatus::Paused;
                        }
                    }
                    for task in &mut q.uploads {
                        if task.status == UploadStatus::Uploading
                            || task.status == UploadStatus::Reconnecting
                        {
                            task.status = UploadStatus::Paused;
                        }
                    }
                    self.download.manager.queue = q.downloads;
                    self.upload.manager.queue = q.uploads;
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
            selected_files: HashSet::new(),
            state,
            status: "Loading...".to_string(),
            should_quit: false,
            storage_quota: None,
            theme_color: ratatui::style::Color::Cyan,
            input_mode: InputMode::Normal,
            token_refreshed_at: std::time::Instant::now(),
            is_refreshing_token: false,
            clipboard: None,
            new_folder_buffer: String::new(),
            download: DownloadState::default(),
            upload: UploadState::default(),
            rename: RenameState::default(),
            search: SearchState::default(),
            preview: PreviewContext::default(),
            nav: NavigationState::default(),
            trash: TrashState::default(),
        }
    }

    /// Returns the current folder ID/path
    pub fn current_folder_id(&self) -> &str {
        &self.nav.current_path
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_initialization() {
        let app = App::new();
        assert!(app.clipboard.is_none());
        assert_eq!(app.current_folder_id(), "virtual_root");
    }

    #[test]
    fn test_clipboard_copy_and_move() {
        let mut app = App::new();
        app.clipboard = Some(Clipboard {
            action: ClipboardAction::Copy,
            file_ids: vec!["file1".to_string(), "file2".to_string()],
            source_parent_id: "folder123".to_string(),
        });
        assert_eq!(
            app.clipboard.as_ref().unwrap().action,
            ClipboardAction::Copy
        );
        assert_eq!(app.clipboard.as_ref().unwrap().file_ids.len(), 2);
        assert_eq!(
            app.clipboard.as_ref().unwrap().source_parent_id,
            "folder123"
        );

        app.clipboard = Some(Clipboard {
            action: ClipboardAction::Move,
            file_ids: vec!["file3".to_string()],
            source_parent_id: "folder456".to_string(),
        });
        assert_eq!(
            app.clipboard.as_ref().unwrap().action,
            ClipboardAction::Move
        );
        assert_eq!(app.clipboard.as_ref().unwrap().file_ids.len(), 1);
    }

    #[test]
    fn test_new_folder_state() {
        let mut app = App::new();
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.new_folder_buffer.is_empty());

        app.input_mode = InputMode::NewFolderModal;
        app.new_folder_buffer.push_str("Documents");
        assert_eq!(app.input_mode, InputMode::NewFolderModal);
        assert_eq!(app.new_folder_buffer, "Documents");
    }
}
