use crate::drive::{api::fetch_files, api::fetch_quota, api::upload_file};
use crate::drive::{auth, models, trash, upload};
use crate::tui::preview::trigger_preview;
use crate::tui::state::{self, Action, App, Event};
use crossterm::event::KeyCode;
use reqwest::Client;
use tokio::sync::mpsc;

pub fn handle_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match app.input_mode {
        state::InputMode::TrashView => match key.code {
            KeyCode::Esc => {
                app.input_mode = state::InputMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !app.trashed_files.is_empty() {
                    let i = match app.trash_state.selected() {
                        Some(i) => {
                            if i >= app.trashed_files.len() - 1 {
                                app.trashed_files.len() - 1
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    app.trash_state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !app.trashed_files.is_empty() {
                    let i = match app.trash_state.selected() {
                        Some(i) => i.saturating_sub(1),
                        None => 0,
                    };
                    app.trash_state.select(Some(i));
                }
            }
            KeyCode::Char('r') => {
                if let Some(i) = app.trash_state.selected() {
                    if let Some(file) = app.trashed_files.get(i).cloned() {
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
                if app.trash_state.selected().is_some() {
                    app.input_mode = state::InputMode::TrashDeleteConfirmModal;
                }
            }
            KeyCode::Char('X') if !app.trashed_files.is_empty() => {
                app.input_mode = state::InputMode::TrashDeleteAllConfirmModal;
            }
            _ => {}
        },
        state::InputMode::TrashDeleteConfirmModal => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if let Some(i) = app.trash_state.selected() {
                    if let Some(file) = app.trashed_files.get(i).cloned() {
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
        },
        state::InputMode::TrashDeleteAllConfirmModal => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                app.status = "Emptying trash...".into();
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                let files = app.trashed_files.clone();
                tokio::spawn(async move {
                    trash::empty_trash(c, t, files, txc).await;
                });
                app.input_mode = state::InputMode::TrashView;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.input_mode = state::InputMode::TrashView;
            }
            _ => {}
        },
        state::InputMode::DeleteConfirmModal => match key.code {
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
                        crate::drive::api::trash_file(c.clone(), t.clone(), file.id, txc.clone())
                            .await;
                    }
                });

                app.input_mode = state::InputMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.input_mode = state::InputMode::Normal;
            }
            _ => {}
        },
        state::InputMode::DownloadConfirmModal => match key.code {
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
        },

        state::InputMode::UploadTrackerView => match key.code {
            KeyCode::Esc => {
                app.input_mode = state::InputMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let i = match app.ul_manager.state.selected() {
                    Some(i) => {
                        if i >= app.ul_manager.queue.len().saturating_sub(1) {
                            i
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                app.ul_manager.state.select(Some(i));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = match app.ul_manager.state.selected() {
                    Some(i) => {
                        if i == 0 {
                            0
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                app.ul_manager.state.select(Some(i));
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(i) = app.ul_manager.state.selected() {
                    if i < app.ul_manager.queue.len() {
                        let task = &app.ul_manager.queue[i];
                        if task.status == models::UploadStatus::Uploading {
                            if let Some(handle) = app.active_ul_task.take() {
                                handle.abort();
                            }
                        }
                        app.ul_manager.queue.remove(i);
                        if app.ul_manager.queue.is_empty() {
                            app.ul_manager.state.select(None);
                        } else {
                            app.ul_manager
                                .state
                                .select(Some(i.min(app.ul_manager.queue.len() - 1)));
                        }

                        if let Some(first) = app
                            .ul_manager
                            .queue
                            .iter_mut()
                            .find(|t| t.status == models::UploadStatus::Pending)
                        {
                            first.status = models::UploadStatus::Uploading;
                            let task_clone = first.clone();
                            let client_c = client.clone();
                            let token_c = token.access_token.clone();
                            let tx_c = tx.clone();
                            app.active_ul_task = Some(tokio::spawn(async move {
                                upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                            }));
                        }
                    }
                }
            }
            KeyCode::Char('p') => {
                if let Some(i) = app.ul_manager.state.selected() {
                    if i < app.ul_manager.queue.len()
                        && app.ul_manager.queue[i].status == models::UploadStatus::Uploading
                    {
                        app.ul_manager.queue[i].status = models::UploadStatus::Paused;
                        if let Some(handle) = app.active_ul_task.take() {
                            handle.abort();
                        }

                        if let Some(first) = app
                            .ul_manager
                            .queue
                            .iter_mut()
                            .find(|t| t.status == models::UploadStatus::Pending)
                        {
                            first.status = models::UploadStatus::Uploading;
                            let task_clone = first.clone();
                            let client_c = client.clone();
                            let token_c = token.access_token.clone();
                            let tx_c = tx.clone();
                            app.active_ul_task = Some(tokio::spawn(async move {
                                upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                            }));
                        }
                    }
                }
            }
            KeyCode::Char('r') => {
                if let Some(i) = app.ul_manager.state.selected() {
                    if i < app.ul_manager.queue.len()
                        && app.ul_manager.queue[i].status == models::UploadStatus::Paused
                    {
                        app.ul_manager.queue[i].status = models::UploadStatus::Pending;
                        if app.active_ul_task.is_none() {
                            if let Some(first) = app
                                .ul_manager
                                .queue
                                .iter_mut()
                                .find(|t| t.status == models::UploadStatus::Pending)
                            {
                                first.status = models::UploadStatus::Uploading;
                                let task_clone = first.clone();
                                let client_c = client.clone();
                                let token_c = token.access_token.clone();
                                let tx_c = tx.clone();
                                app.active_ul_task = Some(tokio::spawn(async move {
                                    upload::upload_file_task(client_c, token_c, task_clone, tx_c)
                                        .await;
                                }));
                            }
                        }
                    }
                }
            }
            _ => {}
        },
        state::InputMode::DownloadTrackerView => match key.code {
            KeyCode::Esc => {
                app.input_mode = state::InputMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !app.dl_manager.queue.is_empty() {
                    let i = match app.dl_manager.state.selected() {
                        Some(i) => {
                            if i >= app.dl_manager.queue.len() - 1 {
                                app.dl_manager.queue.len() - 1
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    app.dl_manager.state.select(Some(i));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if !app.dl_manager.queue.is_empty() {
                    let i = match app.dl_manager.state.selected() {
                        Some(i) => i.saturating_sub(1),
                        None => 0,
                    };
                    app.dl_manager.state.select(Some(i));
                }
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(i) = app.dl_manager.state.selected() {
                    if i < app.dl_manager.queue.len() {
                        let item = &app.dl_manager.queue[i];
                        if item.status == models::DownloadStatus::Downloading {
                            if let Some(task) = app.active_dl_task.take() {
                                task.abort();
                            }
                        }
                        app.dl_manager.queue.remove(i);

                        // Start next if we aborted the active one
                        if app.active_dl_task.is_none() {
                            if let Some(first) = app
                                .dl_manager
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
                                app.active_dl_task = Some(tokio::spawn(async move {
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
                if let Some(i) = app.dl_manager.state.selected() {
                    if let Some(item) = app.dl_manager.queue.get_mut(i) {
                        if item.status == models::DownloadStatus::Downloading {
                            item.status = models::DownloadStatus::Paused;
                            if let Some(task) = app.active_dl_task.take() {
                                task.abort();
                            }
                            app.status = "Download paused.".into();

                            // Start next pending if available
                            if let Some(first) = app
                                .dl_manager
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
                                app.active_dl_task = Some(tokio::spawn(async move {
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
                if let Some(i) = app.dl_manager.state.selected() {
                    if let Some(item) = app.dl_manager.queue.get_mut(i) {
                        if item.status == models::DownloadStatus::Paused {
                            item.status = models::DownloadStatus::Pending;
                            app.status = "Download queued for resume.".into();

                            if app.active_dl_task.is_none() {
                                item.status = models::DownloadStatus::Downloading;
                                let c = client.clone();
                                let t = token.access_token.clone();
                                let txc = tx.clone();
                                let f = item.file.clone();
                                let start_bytes = item.downloaded_bytes;
                                app.active_dl_task = Some(tokio::spawn(async move {
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
        },
        state::InputMode::UploadModal => match key.code {
            KeyCode::Esc => {
                app.input_mode = state::InputMode::Normal;
            }
            KeyCode::Tab => {
                app.upload_input_idx = (app.upload_input_idx + 1) % 2;
            }
            KeyCode::Enter => {
                let path_str = if app.path_names.len() == 1 {
                    "/".to_string()
                } else {
                    format!("/{}", app.path_names[1..].join("/"))
                };
                let parent = if app.upload_target_id == path_str {
                    app.current_path.clone()
                } else {
                    app.upload_target_id.clone()
                };
                let path = app.upload_local_path.clone();
                app.upload_progress = Some((0, 0, 0.0));
                app.input_mode = state::InputMode::Normal;
                app.upload_local_path.clear();
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                tokio::spawn(async move {
                    upload_file(c, t, parent, path, txc).await;
                });
            }
            KeyCode::Backspace => {
                if app.upload_input_idx == 0 {
                    app.upload_target_id.pop();
                } else {
                    app.upload_local_path.pop();
                }
            }
            KeyCode::Char(c) => {
                if app.upload_input_idx == 0 {
                    app.upload_target_id.push(c);
                } else {
                    app.upload_local_path.push(c);
                }
            }
            _ => {}
        },
        state::InputMode::Normal => {
            if app.search_mode {
                match key.code {
                    KeyCode::Esc => {
                        app.search_mode = false;
                    }
                    KeyCode::Enter => {
                        app.search_mode = false;
                    }
                    KeyCode::Backspace => {
                        app.search_query.pop();
                        let sq = app.search_query.clone().replace("'", "\\'");
                        if sq.is_empty() {
                            if app.current_path == "virtual_root" {
                                app.files = vec![
                                    models::DriveFile {
                                        id: "root".to_string(),
                                        name: "My Drive".to_string(),
                                        mime_type: "application/vnd.google-apps.folder".to_string(),
                                    },
                                    models::DriveFile {
                                        id: "shared_with_me".to_string(),
                                        name: "Shared with me".to_string(),
                                        mime_type: "application/vnd.google-apps.folder".to_string(),
                                    },
                                ];
                            } else {
                                let c = client.clone();
                                let t = token.access_token.clone();
                                let txc = tx.clone();
                                let current = app.current_path.clone();
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
                        app.search_query.push(c);
                        let cl = client.clone();
                        let t = token.access_token.clone();
                        let sq = app.search_query.clone().replace("'", "\\'");
                        let txc = tx.clone();
                        tokio::spawn(async move {
                            let q = format!("name contains '{}' and trashed = false", sq);
                            fetch_files(cl, t, q, txc).await;
                        });
                    }
                    _ => {}
                }
            } else {
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
                                app.preview_mode = match app.preview_mode {
                                    state::PreviewMode::Hidden => state::PreviewMode::Default,
                                    state::PreviewMode::Default => {
                                        state::PreviewMode::ForceMetadata
                                    }
                                    state::PreviewMode::ForceMetadata => state::PreviewMode::Hidden,
                                };
                            } else {
                                app.preview_mode = match app.preview_mode {
                                    state::PreviewMode::Hidden => state::PreviewMode::Default,
                                    _ => state::PreviewMode::Hidden,
                                };
                            }
                        } else {
                            app.preview_mode = match app.preview_mode {
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
                        let parent_id = app.current_path.clone();
                        if parent_id == "virtual_root" {
                            app.files = vec![
                                models::DriveFile {
                                    id: "root".to_string(),
                                    name: "My Drive".to_string(),
                                    mime_type: "application/vnd.google-apps.folder".to_string(),
                                },
                                models::DriveFile {
                                    id: "shared_with_me".to_string(),
                                    name: "Shared with me".to_string(),
                                    mime_type: "application/vnd.google-apps.folder".to_string(),
                                },
                            ];
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
                    KeyCode::Char('/') => {
                        app.search_mode = true;
                        app.search_query.clear();
                    }
                    KeyCode::Char('v') | KeyCode::Char('m') => {
                        if let Some(file) = app.selected_file().cloned() {
                            if !file.mime_type.ends_with("folder") {
                                app.status = format!("Starting mpv for {}...", file.name);
                                let url = format!(
                                    "https://www.googleapis.com/drive/v3/files/{}?alt=media",
                                    file.id
                                );
                                let token_str = token.access_token.clone();
                                tokio::spawn(async move {
                                    let _ = tokio::process::Command::new("mpv")
                                        .arg(&url)
                                        .arg(format!(
                                            "--http-header-fields=Authorization: Bearer {}",
                                            token_str
                                        ))
                                        .stdout(std::process::Stdio::null())
                                        .stderr(std::process::Stdio::null())
                                        .spawn();
                                });
                            }
                        }
                    }
                    KeyCode::Enter | KeyCode::Char('l') => {
                        if let Some(file) = app.selected_file().cloned() {
                            if file.mime_type == "application/vnd.google-apps.folder" {
                                app.history.push(app.current_path.clone());
                                app.path_names.push(file.name.clone());
                                app.current_path = file.id.clone();
                                app.status = format!("Loading folder: {}...", file.name);
                                app.files.clear();

                                let c = client.clone();
                                let t = token.access_token.clone();
                                let fid = file.id.clone();
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
                        if let Some(parent_id) = app.history.pop() {
                            app.path_names.pop();
                            app.current_path = parent_id.clone();
                            app.status = "Going back...".into();
                            app.files.clear();
                            app.selected_files.clear();

                            if parent_id == "virtual_root" {
                                app.files = vec![
                                    models::DriveFile {
                                        id: "root".to_string(),
                                        name: "My Drive".to_string(),
                                        mime_type: "application/vnd.google-apps.folder".to_string(),
                                    },
                                    models::DriveFile {
                                        id: "shared_with_me".to_string(),
                                        name: "Shared with me".to_string(),
                                        mime_type: "application/vnd.google-apps.folder".to_string(),
                                    },
                                ];
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
                        if app.path_names.len() == 1 {
                            app.upload_target_id = "/".to_string();
                        } else {
                            app.upload_target_id = format!("/{}", app.path_names[1..].join("/"));
                        }
                        app.upload_input_idx = 1;
                        let home = directories::UserDirs::new()
                            .map(|u| u.home_dir().to_string_lossy().to_string())
                            .unwrap_or_else(|| "/".to_string());
                        app.upload_local_path = if home.ends_with('/') {
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
                            if file.mime_type != "application/vnd.google-apps.folder" {
                                let _ = tx.try_send(Event::SuspendAndEdit(file));
                            } else {
                                app.status = "Cannot edit folders.".into();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
