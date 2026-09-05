use crate::drive::api::{create_folder, fetch_files, fetch_quota, rename_file, upload_file};
use crate::drive::models::{self, DriveFile};
use crate::drive::{auth, trash, upload};
use crate::tui::preview::{is_editable_text, trigger_preview};
use crate::tui::state::{self, Action, App, Event};
use crossterm::event::{KeyCode, KeyEvent};
use reqwest::Client;
use tokio::sync::mpsc;

/// Main keyboard event dispatcher. Routes input events to mode-specific handlers.
pub fn handle_input(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match app.input_mode {
        state::InputMode::TrashView => handle_trash_view_keys(app, key, client, token, tx),
        state::InputMode::TrashDeleteConfirmModal => {
            handle_trash_delete_confirm_keys(app, key, client, token, tx);
        }
        state::InputMode::TrashDeleteAllConfirmModal => {
            handle_trash_empty_confirm_keys(app, key, client, token, tx);
        }
        state::InputMode::DeleteConfirmModal => {
            handle_delete_confirm_keys(app, key, client, token, tx);
        }
        state::InputMode::DownloadConfirmModal => {
            handle_download_confirm_keys(app, key, tx);
        }
        state::InputMode::UploadTrackerView => {
            handle_upload_tracker_keys(app, key, client, token, tx);
        }
        state::InputMode::DownloadTrackerView => {
            handle_download_tracker_keys(app, key, client, token, tx);
        }
        state::InputMode::UploadModal => {
            handle_upload_modal_keys(app, key, client, token, tx);
        }
        state::InputMode::RenameModal => {
            handle_rename_modal_keys(app, key, client, token, tx);
        }
        state::InputMode::NewFolderModal => {
            handle_new_folder_modal_keys(app, key, client, token, tx);
        }
        state::InputMode::Normal => {
            if app.search.active {
                handle_search_keys(app, key, client, token, tx);
            } else {
                handle_normal_keys(app, key, client, token, tx);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rename Modal Handler
// ---------------------------------------------------------------------------

fn handle_rename_modal_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Esc => {
            app.rename.buffer.clear();
            app.rename.target_id.clear();
            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.rename.buffer.pop();
        }
        KeyCode::Enter => {
            let file_id = std::mem::take(&mut app.rename.target_id);
            let new_name = std::mem::take(&mut app.rename.buffer);
            app.input_mode = state::InputMode::Normal;

            if file_id.is_empty() {
                return;
            }

            app.status = format!("Renaming {}...", new_name);
            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();
            tokio::spawn(async move {
                rename_file(c, t, file_id, new_name, txc).await;
            });
        }
        KeyCode::Char(c) => {
            app.rename.buffer.push(c);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// New Folder Modal Handler
// ---------------------------------------------------------------------------

fn handle_new_folder_modal_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Esc => {
            app.new_folder_buffer.clear();
            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Backspace => {
            app.new_folder_buffer.pop();
        }
        KeyCode::Enter => {
            let folder_name = std::mem::take(&mut app.new_folder_buffer)
                .trim()
                .to_string();
            app.input_mode = state::InputMode::Normal;

            if folder_name.is_empty() {
                app.status = "Folder creation cancelled: name cannot be empty.".into();
                return;
            }

            if app.current_folder_id() == "shared_with_me" {
                app.status =
                    "Cannot create folder in 'Shared with me'. Please open a subfolder first."
                        .into();
                return;
            }

            let parent_id = if app.current_folder_id() == "virtual_root" {
                "root".to_string()
            } else {
                app.current_folder_id().to_string()
            };

            app.status = format!("Creating folder '{}'...", folder_name);
            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();
            tokio::spawn(async move {
                create_folder(c, t, folder_name, parent_id, txc).await;
            });
        }
        KeyCode::Char(c) => {
            app.new_folder_buffer.push(c);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Trash Mode Handlers
// ---------------------------------------------------------------------------

fn handle_trash_view_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if !app.trash.files.is_empty() {
                let i = match app.trash.state.selected() {
                    Some(i) => {
                        if i >= app.trash.files.len() - 1 {
                            app.trash.files.len() - 1
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                app.trash.state.select(Some(i));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if !app.trash.files.is_empty() {
                let i = match app.trash.state.selected() {
                    Some(i) => i.saturating_sub(1),
                    None => 0,
                };
                app.trash.state.select(Some(i));
            }
        }
        KeyCode::Char('r') => {
            if let Some(i) = app.trash.state.selected() {
                if let Some(file) = app.trash.files.get(i).cloned() {
                    app.status = format!("Restoring {}...", file.name);
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    tokio::spawn(async move {
                        trash::restore_file(c, t, file.id, txc).await;
                    });
                }
            }
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if app.trash.state.selected().is_some() {
                app.input_mode = state::InputMode::TrashDeleteConfirmModal;
            }
        }
        KeyCode::Char('X') if !app.trash.files.is_empty() => {
            app.input_mode = state::InputMode::TrashDeleteAllConfirmModal;
        }
        _ => {}
    }
}

fn handle_trash_delete_confirm_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            if let Some(i) = app.trash.state.selected() {
                if let Some(file) = app.trash.files.get(i).cloned() {
                    app.status = format!("Deleting permanently {}...", file.name);
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    tokio::spawn(async move {
                        trash::delete_permanently(c, t, file.id, txc).await;
                    });
                }
            }
            app.input_mode = state::InputMode::TrashView;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = state::InputMode::TrashView;
        }
        _ => {}
    }
}

fn handle_trash_empty_confirm_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            app.status = "Emptying trash...".into();
            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();
            let files = app.trash.files.clone();
            tokio::spawn(async move {
                trash::empty_trash(c, t, files, txc).await;
            });
            app.input_mode = state::InputMode::TrashView;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = state::InputMode::TrashView;
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Confirmation Modal Handlers
// ---------------------------------------------------------------------------

fn handle_delete_confirm_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            let targets: Vec<models::DriveFile> = if !app.selected_files.is_empty() {
                app.files
                    .iter()
                    .filter(|f| app.selected_files.contains(&f.id))
                    .cloned()
                    .collect()
            } else if let Some(file) = app.selected_file().cloned() {
                vec![file]
            } else {
                vec![]
            };

            app.status = format!("Trashing {} files...", targets.len());
            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();

            // Remove from UI immediately
            app.files
                .retain(|f| !targets.iter().any(|tf| tf.id == f.id));
            app.selected_files.clear();

            tokio::spawn(async move {
                for file in targets {
                    crate::drive::api::trash_file(c.clone(), t.clone(), file.id, txc.clone()).await;
                }
            });

            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = state::InputMode::Normal;
        }
        _ => {}
    }
}

fn handle_download_confirm_keys(app: &mut App, key: KeyEvent, tx: &mpsc::Sender<Event>) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            let targets: Vec<models::DriveFile> = if !app.selected_files.is_empty() {
                app.files
                    .iter()
                    .filter(|f| app.selected_files.contains(&f.id))
                    .cloned()
                    .collect()
            } else if let Some(file) = app.selected_file().cloned() {
                vec![file]
            } else {
                vec![]
            };

            app.status = format!("Queued {} files for download.", targets.len());
            app.selected_files.clear();
            let txc = tx.clone();
            tokio::spawn(async move {
                let _ = txc
                    .send(Event::Action(Action::QueueDownloads(targets)))
                    .await;
            });
            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.input_mode = state::InputMode::Normal;
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Transfer Queue Trackers
// ---------------------------------------------------------------------------

fn handle_upload_tracker_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let i = match app.upload.manager.state.selected() {
                Some(i) => {
                    if i >= app.upload.manager.queue.len().saturating_sub(1) {
                        i
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            app.upload.manager.state.select(Some(i));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let i = match app.upload.manager.state.selected() {
                Some(i) => {
                    if i == 0 {
                        0
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            app.upload.manager.state.select(Some(i));
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if let Some(i) = app.upload.manager.state.selected() {
                if i < app.upload.manager.queue.len() {
                    let task = &app.upload.manager.queue[i];
                    if task.status == models::UploadStatus::Uploading
                        || task.status == models::UploadStatus::Reconnecting
                    {
                        if let Some(handle) = app.upload.active_task.take() {
                            handle.abort();
                        }
                    }
                    app.upload.manager.queue.remove(i);
                    if app.upload.manager.queue.is_empty() {
                        app.upload.manager.state.select(None);
                    } else {
                        app.upload
                            .manager
                            .state
                            .select(Some(i.min(app.upload.manager.queue.len() - 1)));
                    }

                    if let Some(first) = app
                        .upload
                        .manager
                        .queue
                        .iter_mut()
                        .find(|t| t.status == models::UploadStatus::Pending)
                    {
                        first.status = models::UploadStatus::Uploading;
                        let task_clone = first.clone();
                        let client_c = client.clone();
                        let token_c = token.access_token.clone();
                        let tx_c = tx.clone();
                        app.upload.active_task = Some(tokio::spawn(async move {
                            upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                        }));
                    }
                }
            }
        }
        KeyCode::Char('p') => {
            if let Some(i) = app.upload.manager.state.selected() {
                if i < app.upload.manager.queue.len()
                    && (app.upload.manager.queue[i].status == models::UploadStatus::Uploading
                        || app.upload.manager.queue[i].status == models::UploadStatus::Reconnecting)
                {
                    app.upload.manager.queue[i].status = models::UploadStatus::Paused;
                    if let Some(handle) = app.upload.active_task.take() {
                        handle.abort();
                    }

                    if let Some(first) = app
                        .upload
                        .manager
                        .queue
                        .iter_mut()
                        .find(|t| t.status == models::UploadStatus::Pending)
                    {
                        first.status = models::UploadStatus::Uploading;
                        let task_clone = first.clone();
                        let client_c = client.clone();
                        let token_c = token.access_token.clone();
                        let tx_c = tx.clone();
                        app.upload.active_task = Some(tokio::spawn(async move {
                            upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                        }));
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            if let Some(i) = app.upload.manager.state.selected() {
                if i < app.upload.manager.queue.len()
                    && app.upload.manager.queue[i].status == models::UploadStatus::Paused
                {
                    app.upload.manager.queue[i].status = models::UploadStatus::Pending;
                    if app.upload.active_task.is_none() {
                        if let Some(first) = app
                            .upload
                            .manager
                            .queue
                            .iter_mut()
                            .find(|t| t.status == models::UploadStatus::Pending)
                        {
                            first.status = models::UploadStatus::Uploading;
                            let task_clone = first.clone();
                            let client_c = client.clone();
                            let token_c = token.access_token.clone();
                            let tx_c = tx.clone();
                            app.upload.active_task = Some(tokio::spawn(async move {
                                upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                            }));
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_download_tracker_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if !app.download.manager.queue.is_empty() {
                let i = match app.download.manager.state.selected() {
                    Some(i) => {
                        if i >= app.download.manager.queue.len() - 1 {
                            app.download.manager.queue.len() - 1
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                app.download.manager.state.select(Some(i));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if !app.download.manager.queue.is_empty() {
                let i = match app.download.manager.state.selected() {
                    Some(i) => i.saturating_sub(1),
                    None => 0,
                };
                app.download.manager.state.select(Some(i));
            }
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if let Some(i) = app.download.manager.state.selected() {
                if i < app.download.manager.queue.len() {
                    let item = &app.download.manager.queue[i];
                    if item.status == models::DownloadStatus::Downloading
                        || item.status == models::DownloadStatus::Reconnecting
                    {
                        if let Some(task) = app.download.active_task.take() {
                            task.abort();
                        }
                    }
                    app.download.manager.queue.remove(i);

                    if app.download.active_task.is_none() {
                        if let Some(first) = app
                            .download
                            .manager
                            .queue
                            .iter_mut()
                            .find(|t| t.status != models::DownloadStatus::Paused)
                        {
                            first.status = models::DownloadStatus::Downloading;
                            let c = client.clone();
                            let t = token.access_token.clone();
                            let txc = tx.clone();
                            let f = first.file.clone();
                            let start_bytes = first.downloaded_bytes;
                            app.download.active_task = Some(tokio::spawn(async move {
                                crate::drive::download::download_file_ranged(
                                    c,
                                    t,
                                    f,
                                    start_bytes,
                                    txc,
                                )
                                .await;
                            }));
                        }
                    }
                }
            }
        }
        KeyCode::Char('p') => {
            if let Some(i) = app.download.manager.state.selected() {
                if let Some(item) = app.download.manager.queue.get_mut(i) {
                    if item.status == models::DownloadStatus::Downloading
                        || item.status == models::DownloadStatus::Reconnecting
                    {
                        item.status = models::DownloadStatus::Paused;
                        if let Some(task) = app.download.active_task.take() {
                            task.abort();
                        }
                        app.status = "Download paused.".into();

                        if let Some(first) = app
                            .download
                            .manager
                            .queue
                            .iter_mut()
                            .find(|t| t.status == models::DownloadStatus::Pending)
                        {
                            first.status = models::DownloadStatus::Downloading;
                            let c = client.clone();
                            let t = token.access_token.clone();
                            let txc = tx.clone();
                            let f = first.file.clone();
                            let start_bytes = first.downloaded_bytes;
                            app.download.active_task = Some(tokio::spawn(async move {
                                crate::drive::download::download_file_ranged(
                                    c,
                                    t,
                                    f,
                                    start_bytes,
                                    txc,
                                )
                                .await;
                            }));
                        }
                    }
                }
            }
        }
        KeyCode::Char('r') => {
            if let Some(i) = app.download.manager.state.selected() {
                if let Some(item) = app.download.manager.queue.get_mut(i) {
                    if item.status == models::DownloadStatus::Paused {
                        item.status = models::DownloadStatus::Pending;
                        app.status = "Download queued for resume.".into();

                        if app.download.active_task.is_none() {
                            item.status = models::DownloadStatus::Downloading;
                            let c = client.clone();
                            let t = token.access_token.clone();
                            let txc = tx.clone();
                            let f = item.file.clone();
                            let start_bytes = item.downloaded_bytes;
                            app.download.active_task = Some(tokio::spawn(async move {
                                crate::drive::download::download_file_ranged(
                                    c,
                                    t,
                                    f,
                                    start_bytes,
                                    txc,
                                )
                                .await;
                            }));
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Upload Modal Handler
// ---------------------------------------------------------------------------

fn handle_upload_modal_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Esc => {
            app.input_mode = state::InputMode::Normal;
        }
        KeyCode::Tab => {
            app.upload.input_idx = (app.upload.input_idx + 1) % 2;
        }
        KeyCode::Enter => {
            let path_str = if app.nav.path_names.len() == 1 {
                "/".to_string()
            } else {
                format!("/{}", app.nav.path_names[1..].join("/"))
            };

            if app.nav.current_path == "shared_with_me" && app.upload.target_id == path_str {
                app.status =
                    "Cannot upload directly to 'Shared with me' root. Please open a shared folder first."
                        .into();
                app.input_mode = state::InputMode::Normal;
                return;
            }

            let parent = if app.upload.target_id == path_str {
                if app.nav.current_path == "virtual_root" {
                    "root".to_string()
                } else {
                    app.nav.current_path.clone()
                }
            } else {
                app.upload.target_id.clone()
            };

            let path = app.upload.local_path.trim().to_string();
            if path.is_empty() {
                app.status = "Local path cannot be empty.".into();
                app.input_mode = state::InputMode::Normal;
                return;
            }

            app.upload.progress = Some((0, 0, 0.0));
            app.input_mode = state::InputMode::Normal;
            app.upload.local_path.clear();
            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();
            tokio::spawn(async move {
                upload_file(c, t, parent, path, txc).await;
            });
        }
        KeyCode::Backspace => {
            if app.upload.input_idx == 0 {
                app.upload.target_id.pop();
            } else {
                app.upload.local_path.pop();
            }
        }
        KeyCode::Char(c) => {
            if app.upload.input_idx == 0 {
                app.upload.target_id.push(c);
            } else {
                app.upload.local_path.push(c);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Search Handler
// ---------------------------------------------------------------------------

fn handle_search_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.search.active = false;
        }
        KeyCode::Backspace => {
            app.search.query.pop();
            let sq = app.search.query.clone().replace('\'', "\\'");
            if sq.is_empty() {
                if app.nav.current_path == "virtual_root" {
                    app.files = DriveFile::virtual_root_items();
                } else {
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    let current = app.nav.current_path.clone();
                    tokio::spawn(async move {
                        let q = if current == "shared_with_me" {
                            "sharedWithMe = true and trashed = false".to_string()
                        } else {
                            format!("'{}' in parents and trashed = false", current)
                        };
                        fetch_files(c, t, q, txc).await;
                    });
                }
            } else {
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                tokio::spawn(async move {
                    let q = format!("name contains '{}' and trashed = false", sq);
                    fetch_files(c, t, q, txc).await;
                });
            }
        }
        KeyCode::Char(c) => {
            app.search.query.push(c);
            let cl = client.clone();
            let t = token.access_token.clone();
            let sq = app.search.query.clone().replace('\'', "\\'");
            let txc = tx.clone();
            tokio::spawn(async move {
                let q = format!("name contains '{}' and trashed = false", sq);
                fetch_files(cl, t, q, txc).await;
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Normal Navigation & Shortcuts
// ---------------------------------------------------------------------------

fn handle_normal_keys(
    app: &mut App,
    key: KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match key.code {
        KeyCode::Char('q') => {
            app.save_queues();
            app.should_quit = true;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.next();
            trigger_preview(app, client, &token.access_token, tx);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.previous();
            trigger_preview(app, client, &token.access_token, tx);
        }
        KeyCode::Char('p') => {
            if let Some(file) = app.selected_file() {
                if file.mime_type.starts_with("image/") {
                    app.preview.mode = match app.preview.mode {
                        state::PreviewMode::Hidden => state::PreviewMode::Default,
                        state::PreviewMode::Default => state::PreviewMode::ForceMetadata,
                        state::PreviewMode::ForceMetadata => state::PreviewMode::Hidden,
                    };
                } else {
                    app.preview.mode = match app.preview.mode {
                        state::PreviewMode::Hidden => state::PreviewMode::Default,
                        _ => state::PreviewMode::Hidden,
                    };
                }
            } else {
                app.preview.mode = match app.preview.mode {
                    state::PreviewMode::Hidden => state::PreviewMode::Default,
                    _ => state::PreviewMode::Hidden,
                };
            }
            trigger_preview(app, client, &token.access_token, tx);
        }
        KeyCode::Char('r') => {
            app.status = "Refreshing...".into();
            app.files.clear();
            app.selected_files.clear();
            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();
            let parent_id = app.nav.current_path.clone();
            if parent_id == "virtual_root" {
                app.files = DriveFile::virtual_root_items();
                app.status = "Virtual Root loaded.".to_string();
            } else {
                tokio::spawn(async move {
                    let q = if parent_id == "shared_with_me" {
                        "sharedWithMe = true and trashed = false".to_string()
                    } else {
                        format!("'{}' in parents and trashed = false", parent_id)
                    };
                    fetch_files(c.clone(), t.clone(), q, txc.clone()).await;
                    fetch_quota(c, t, txc).await;
                });
            }
        }
        KeyCode::Char('n') => {
            if app.current_folder_id() == "shared_with_me" {
                app.status =
                    "Cannot create folder in 'Shared with me'. Please open a subfolder first."
                        .into();
                return;
            }
            app.new_folder_buffer.clear();
            app.input_mode = state::InputMode::NewFolderModal;
        }
        KeyCode::Char('c') => {
            if let Some(file) = app.selected_file().cloned() {
                app.rename.target_id = file.id;
                app.rename.buffer = file.name;
                app.input_mode = state::InputMode::RenameModal;
            }
        }
        KeyCode::Char('T') => {
            app.input_mode = state::InputMode::TrashView;
            app.status = "Loading trash...".into();
            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();
            tokio::spawn(async move {
                trash::fetch_trash(c, t, txc).await;
            });
        }
        KeyCode::Char('y') => {
            let file_ids: Vec<String> = if !app.selected_files.is_empty() {
                app.selected_files.iter().cloned().collect()
            } else if let Some(file) = app.selected_file() {
                if file.id == "root" || file.id == "shared_with_me" {
                    app.status = "Cannot copy virtual root items.".into();
                    return;
                }
                vec![file.id.clone()]
            } else {
                Vec::new()
            };

            if !file_ids.is_empty() {
                let count = file_ids.len();
                app.clipboard = Some(state::Clipboard {
                    action: state::ClipboardAction::Copy,
                    file_ids,
                    source_parent_id: app.current_folder_id().to_string(),
                });
                app.selected_files.clear();
                app.status = format!(
                    "Copied {} {}.",
                    count,
                    if count == 1 { "file" } else { "files" }
                );
            } else {
                app.status = "No file selected to copy.".into();
            }
        }
        KeyCode::Char('m') => {
            let file_ids: Vec<String> = if !app.selected_files.is_empty() {
                app.selected_files.iter().cloned().collect()
            } else if let Some(file) = app.selected_file() {
                if file.id == "root" || file.id == "shared_with_me" {
                    app.status = "Cannot move virtual root items.".into();
                    return;
                }
                vec![file.id.clone()]
            } else {
                Vec::new()
            };

            if !file_ids.is_empty() {
                let count = file_ids.len();
                app.clipboard = Some(state::Clipboard {
                    action: state::ClipboardAction::Move,
                    file_ids,
                    source_parent_id: app.current_folder_id().to_string(),
                });
                app.selected_files.clear();
                app.status = format!(
                    "Cut {} {}.",
                    count,
                    if count == 1 { "file" } else { "files" }
                );
            } else {
                app.status = "No file selected to move.".into();
            }
        }
        KeyCode::Char('P') => {
            let target_folder_id = app.current_folder_id().to_string();
            if target_folder_id == "virtual_root" {
                app.status = "Cannot paste into virtual root. Please open a folder first.".into();
                return;
            }
            if target_folder_id == "shared_with_me" {
                app.status =
                    "Cannot paste into 'Shared with me' root. Please open a subfolder first."
                        .into();
                return;
            }
            if let Some(clipboard) = app.clipboard.clone() {
                if clipboard.action == state::ClipboardAction::Move
                    && clipboard.source_parent_id == target_folder_id
                {
                    app.status = "Source and target folders are identical.".into();
                    return;
                }
                let count = clipboard.file_ids.len();
                let action_name = match clipboard.action {
                    state::ClipboardAction::Copy => "Copying",
                    state::ClipboardAction::Move => "Moving",
                };
                app.status = format!(
                    "{} {} {}...",
                    action_name,
                    count,
                    if count == 1 { "item" } else { "items" }
                );

                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();

                tokio::spawn(async move {
                    crate::drive::api::process_paste(c, t, clipboard, target_folder_id, txc).await;
                });
            } else {
                app.status = "Clipboard is empty.".into();
            }
        }
        KeyCode::Char('v') => {
            if let Some(file) = app.selected_file().cloned() {
                if !file.mime_type.ends_with("folder") {
                    let resume_time = file
                        .app_properties
                        .as_ref()
                        .and_then(|props| props.get("mpv_resume_time").cloned());

                    if let Some(ref t) = resume_time {
                        app.status = format!("Resuming {} at {}s...", file.name, t);
                    } else {
                        app.status = format!("Starting mpv for {}...", file.name);
                    }

                    let url = format!(
                        "https://www.googleapis.com/drive/v3/files/{}?alt=media",
                        file.id
                    );
                    let token_str = token.access_token.clone();
                    let client_c = client.clone();
                    let txc = tx.clone();
                    let file_id = file.id;

                    tokio::spawn(async move {
                        let mut cmd = tokio::process::Command::new("mpv");
                        cmd.arg(&url)
                            .arg(format!(
                                "--http-header-fields=Authorization: Bearer {}",
                                token_str
                            ))
                            .arg("--term-status-msg=MPV_RESUME_TIME:${=time-pos}");

                        if let Some(ref t) = resume_time {
                            cmd.arg(format!("--start={}", t));
                        }

                        cmd.stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped());

                        if let Ok(output) = cmd.output().await {
                            let stdout_str = String::from_utf8_lossy(&output.stdout);
                            let stderr_str = String::from_utf8_lossy(&output.stderr);
                            let combined = format!("{}\n{}", stdout_str, stderr_str);

                            if let Some(extracted_time) = extract_mpv_resume_time(&combined) {
                                let ext_c = extracted_time.clone();
                                let fid = file_id.clone();
                                let c = client_c.clone();
                                let tok = token_str.clone();

                                tokio::spawn(async move {
                                    let _ =
                                        crate::drive::api::update_resume_time(c, tok, fid, ext_c)
                                            .await;
                                });

                                let _ = txc
                                    .send(Event::Action(Action::UpdateResumeTime(
                                        file_id,
                                        extracted_time,
                                    )))
                                    .await;
                            }
                        }
                    });
                }
            }
        }
        KeyCode::Enter | KeyCode::Char('l') => {
            if let Some(file) = app.selected_file().cloned() {
                if file.mime_type == "application/vnd.google-apps.folder" {
                    app.nav.history.push(app.nav.current_path.clone());
                    app.nav.path_names.push(file.name.clone());
                    app.nav.current_path = file.id.clone();
                    app.status = format!("Loading folder: {}...", file.name);
                    app.files.clear();

                    let c = client.clone();
                    let t = token.access_token.clone();
                    let fid = file.id;
                    let txc = tx.clone();

                    tokio::spawn(async move {
                        let q = if fid == "shared_with_me" {
                            "sharedWithMe = true and trashed = false".to_string()
                        } else {
                            format!("'{}' in parents and trashed = false", fid)
                        };
                        fetch_files(c, t, q, txc).await;
                    });
                } else {
                    app.status = format!("Cannot open file: {}", file.name);
                }
            }
        }
        KeyCode::Char('h') | KeyCode::Backspace => {
            if let Some(parent_id) = app.nav.history.pop() {
                app.nav.path_names.pop();
                app.nav.current_path = parent_id.clone();
                app.status = "Going back...".into();
                app.files.clear();
                app.selected_files.clear();

                if parent_id == "virtual_root" {
                    app.files = DriveFile::virtual_root_items();
                    app.status = "Virtual Root loaded.".to_string();
                } else {
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    tokio::spawn(async move {
                        let q = if parent_id == "shared_with_me" {
                            "sharedWithMe = true and trashed = false".to_string()
                        } else {
                            format!("'{}' in parents and trashed = false", parent_id)
                        };
                        fetch_files(c, t, q, txc).await;
                    });
                }
            } else {
                app.status = "Already at root.".into();
            }
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            if app.selected_file().is_some() {
                app.input_mode = state::InputMode::DeleteConfirmModal;
            }
        }
        KeyCode::Char('d') => {
            if let Some(file) = app.selected_file().cloned() {
                if file.mime_type != "application/vnd.google-apps.folder" {
                    app.input_mode = state::InputMode::DownloadConfirmModal;
                } else {
                    app.status = "Cannot download folders directly yet.".into();
                }
            }
        }
        KeyCode::Char('D') => {
            app.input_mode = state::InputMode::DownloadTrackerView;
        }
        KeyCode::Char('U') => {
            app.input_mode = state::InputMode::UploadTrackerView;
        }
        KeyCode::Char('u') => {
            app.input_mode = state::InputMode::UploadModal;
            if app.nav.path_names.len() == 1 {
                app.upload.target_id = "/".to_string();
            } else {
                app.upload.target_id = format!("/{}", app.nav.path_names[1..].join("/"));
            }
            app.upload.input_idx = 1;
            let home = directories::UserDirs::new()
                .map(|u| u.home_dir().to_string_lossy().to_string())
                .unwrap_or_else(|| "/".to_string());
            app.upload.local_path = if home.ends_with('/') {
                home
            } else {
                format!("{}/", home)
            };
        }
        KeyCode::Char('a') => {
            if let Some(file) = app.selected_file() {
                if app.selected_files.contains(&file.id) {
                    app.selected_files.remove(&file.id.clone());
                } else {
                    app.selected_files.insert(file.id.clone());
                }
            }
            app.next();
            trigger_preview(app, client, &token.access_token, tx);
        }
        KeyCode::Char('A') => {
            app.selected_files.clear();
        }
        KeyCode::Char('e') => {
            if let Some(file) = app.selected_file().cloned() {
                if file.mime_type == "application/vnd.google-apps.folder" {
                    app.status = "Cannot edit folders.".into();
                } else if !is_editable_text(&file.mime_type) {
                    app.status = "Cannot edit binary/media files.".into();
                    let _ = tx.try_send(Event::Action(Action::Message(
                        "Cannot edit binary/media files.".into(),
                    )));
                } else {
                    let _ = tx.try_send(Event::SuspendAndEdit(file));
                }
            }
        }
        _ => {}
    }
}

fn extract_mpv_resume_time(output: &str) -> Option<String> {
    let mut parts = output.split("MPV_RESUME_TIME:").collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    let last = parts.pop()?;
    let time_str: String = last
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();

    if let Ok(secs) = time_str.parse::<f64>() {
        if secs > 0.0 {
            return Some(format!("{:.1}", secs));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mpv_resume_time_single() {
        let out = "AO: [null]\nMPV_RESUME_TIME:124.523\nExiting...";
        assert_eq!(extract_mpv_resume_time(out), Some("124.5".to_string()));
    }

    #[test]
    fn test_extract_mpv_resume_time_multiple() {
        let out = "MPV_RESUME_TIME:0.012MPV_RESUME_TIME:12.345MPV_RESUME_TIME:88.999\nExiting";
        assert_eq!(extract_mpv_resume_time(out), Some("89.0".to_string()));
    }

    #[test]
    fn test_extract_mpv_resume_time_none() {
        assert_eq!(extract_mpv_resume_time("No status message here"), None);
        assert_eq!(extract_mpv_resume_time("MPV_RESUME_TIME:0.0"), None);
    }
}
