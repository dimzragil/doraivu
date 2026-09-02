use crate::api::{fetch_files, fetch_quota, upload_file};
use crate::app::{self, Action, App, Event};
use crate::trigger_preview;
use crate::{api, auth, trash, upload};
use crossterm::{
    event::KeyCode,
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::Client;
use std::io::Stdout;
use tokio::sync::mpsc;

pub fn handle_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    client: &Client,
    token: &auth::Token,
    tx: &mpsc::Sender<Event>,
) {
    match app.input_mode {
        app::InputMode::TrashView => match key.code {
            KeyCode::Esc => {
                app.input_mode = app::InputMode::Normal;
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
                    app.input_mode = app::InputMode::TrashDeleteConfirmModal;
                }
            }
            KeyCode::Char('X') if !app.trashed_files.is_empty() => {
                app.input_mode = app::InputMode::TrashDeleteAllConfirmModal;
            }
            _ => {}
        },
        app::InputMode::TrashDeleteConfirmModal => match key.code {
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
                app.input_mode = app::InputMode::TrashView;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.input_mode = app::InputMode::TrashView;
            }
            _ => {}
        },
        app::InputMode::TrashDeleteAllConfirmModal => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                app.status = "Emptying trash...".into();
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                let files = app.trashed_files.clone();
                tokio::spawn(async move {
                    trash::empty_trash(c, t, files, txc).await;
                });
                app.input_mode = app::InputMode::TrashView;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.input_mode = app::InputMode::TrashView;
            }
            _ => {}
        },
        app::InputMode::DeleteConfirmModal => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                let targets: Vec<crate::app::DriveFile> = if !app.selected_files.is_empty() {
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
                        api::trash_file(c.clone(), t.clone(), file.id, txc.clone()).await;
                    }
                });

                app.input_mode = app::InputMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.input_mode = app::InputMode::Normal;
            }
            _ => {}
        },
        app::InputMode::DownloadConfirmModal => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                let targets: Vec<crate::app::DriveFile> = if !app.selected_files.is_empty() {
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
                        .send(crate::app::Event::Action(
                            crate::app::Action::QueueDownloads(targets),
                        ))
                        .await;
                });
                app.input_mode = app::InputMode::Normal;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.input_mode = app::InputMode::Normal;
            }
            _ => {}
        },

        app::InputMode::UploadTrackerView => match key.code {
            KeyCode::Esc => {
                app.input_mode = app::InputMode::Normal;
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
                        if task.status == crate::app::UploadStatus::Uploading {
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
                            .find(|t| t.status == crate::app::UploadStatus::Pending)
                        {
                            first.status = crate::app::UploadStatus::Uploading;
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
                        && app.ul_manager.queue[i].status == crate::app::UploadStatus::Uploading
                    {
                        app.ul_manager.queue[i].status = crate::app::UploadStatus::Paused;
                        if let Some(handle) = app.active_ul_task.take() {
                            handle.abort();
                        }

                        if let Some(first) = app
                            .ul_manager
                            .queue
                            .iter_mut()
                            .find(|t| t.status == crate::app::UploadStatus::Pending)
                        {
                            first.status = crate::app::UploadStatus::Uploading;
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
                        && app.ul_manager.queue[i].status == crate::app::UploadStatus::Paused
                    {
                        app.ul_manager.queue[i].status = crate::app::UploadStatus::Pending;
                        if app.active_ul_task.is_none() {
                            if let Some(first) = app
                                .ul_manager
                                .queue
                                .iter_mut()
                                .find(|t| t.status == crate::app::UploadStatus::Pending)
                            {
                                first.status = crate::app::UploadStatus::Uploading;
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
        app::InputMode::DownloadTrackerView => match key.code {
            KeyCode::Esc => {
                app.input_mode = app::InputMode::Normal;
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
                        if item.status == crate::app::DownloadStatus::Downloading {
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
                                .find(|t| t.status != crate::app::DownloadStatus::Paused)
                            {
                                first.status = crate::app::DownloadStatus::Downloading;
                                let c = client.clone();
                                let t = token.access_token.clone();
                                let txc = tx.clone();
                                let f = first.file.clone();
                                let start_bytes = first.downloaded_bytes;
                                app.active_dl_task = Some(tokio::spawn(async move {
                                    crate::download::download_file_ranged(
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
                        if item.status == crate::app::DownloadStatus::Downloading {
                            item.status = crate::app::DownloadStatus::Paused;
                            if let Some(task) = app.active_dl_task.take() {
                                task.abort();
                            }
                            app.status = "Download paused.".into();

                            // Start next pending if available
                            if let Some(first) = app
                                .dl_manager
                                .queue
                                .iter_mut()
                                .find(|t| t.status == crate::app::DownloadStatus::Pending)
                            {
                                first.status = crate::app::DownloadStatus::Downloading;
                                let c = client.clone();
                                let t = token.access_token.clone();
                                let txc = tx.clone();
                                let f = first.file.clone();
                                let start_bytes = first.downloaded_bytes;
                                app.active_dl_task = Some(tokio::spawn(async move {
                                    crate::download::download_file_ranged(
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
                        if item.status == crate::app::DownloadStatus::Paused {
                            item.status = crate::app::DownloadStatus::Pending;
                            app.status = "Download queued for resume.".into();

                            if app.active_dl_task.is_none() {
                                item.status = crate::app::DownloadStatus::Downloading;
                                let c = client.clone();
                                let t = token.access_token.clone();
                                let txc = tx.clone();
                                let f = item.file.clone();
                                let start_bytes = item.downloaded_bytes;
                                app.active_dl_task = Some(tokio::spawn(async move {
                                    crate::download::download_file_ranged(
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
        app::InputMode::UploadModal => match key.code {
            KeyCode::Esc => {
                app.input_mode = app::InputMode::Normal;
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
                app.input_mode = app::InputMode::Normal;
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
        app::InputMode::Normal => {
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
                        let c = client.clone();
                        let t = token.access_token.clone();
                        let sq = app.search_query.clone().replace("'", "\\'");
                        let txc = tx.clone();
                        tokio::spawn(async move {
                            let q = if sq.is_empty() {
                                "'root' in parents and trashed = false".to_string()
                            } else {
                                format!("name contains '{}' and trashed = false", sq)
                            };
                            fetch_files(c, t, q, txc).await;
                        });
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
                        trigger_preview(app, &client, &token.access_token, &tx);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.previous();
                        trigger_preview(app, &client, &token.access_token, &tx);
                    }
                    KeyCode::Char('p') => {
                        app.show_preview = !app.show_preview;
                        trigger_preview(app, &client, &token.access_token, &tx);
                    }
                    KeyCode::Char('r') => {
                        app.status = "Refreshing...".into();
                        app.files.clear();
                        app.selected_files.clear();
                        let c = client.clone();
                        let t = token.access_token.clone();
                        let txc = tx.clone();
                        let parent_id = app.current_path.clone();
                        tokio::spawn(async move {
                            let q = format!("'{}' in parents and trashed = false", parent_id);
                            fetch_files(c.clone(), t.clone(), q, txc.clone()).await;
                            fetch_quota(c, t, txc).await;
                        });
                    }
                    KeyCode::Char('T') => {
                        app.input_mode = app::InputMode::TrashView;
                        app.status = "Loading trash...".into();
                        let c = client.clone();
                        let t = token.access_token.clone();
                        let txc = tx.clone();
                        tokio::spawn(async move {
                            crate::trash::fetch_trash(c, t, txc).await;
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
                                app.files.clear(); // Optional: clear while loading

                                let c = client.clone();
                                let t = token.access_token.clone();
                                let fid = file.id.clone();
                                let txc = tx.clone();

                                tokio::spawn(async move {
                                    let q = format!("'{}' in parents and trashed = false", fid);
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

                            let c = client.clone();
                            let t = token.access_token.clone();
                            let txc = tx.clone();

                            tokio::spawn(async move {
                                let q = format!("'{}' in parents and trashed = false", parent_id);
                                fetch_files(c, t, q, txc).await;
                            });
                        } else {
                            app.status = "Already at root.".into();
                        }
                    }
                    KeyCode::Char('x') | KeyCode::Delete => {
                        if app.selected_file().is_some() {
                            app.input_mode = app::InputMode::DeleteConfirmModal;
                        }
                    }
                    KeyCode::Char('d') => {
                        if let Some(file) = app.selected_file().cloned() {
                            if file.mime_type != "application/vnd.google-apps.folder" {
                                app.input_mode = app::InputMode::DownloadConfirmModal;
                            } else {
                                app.status = "Cannot download folders directly yet.".into();
                            }
                        }
                    }
                    KeyCode::Char('D') => {
                        app.input_mode = app::InputMode::DownloadTrackerView;
                    }
                    KeyCode::Char('U') => {
                        app.input_mode = app::InputMode::UploadTrackerView;
                    }
                    KeyCode::Char('u') => {
                        app.input_mode = app::InputMode::UploadModal;
                        if app.path_names.len() == 1 {
                            app.upload_target_id = "/".to_string();
                        } else {
                            app.upload_target_id = format!("/{}", app.path_names[1..].join("/"));
                        }
                        app.upload_input_idx = 1; // focus local path
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
                        trigger_preview(app, &client, &token.access_token, &tx);
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

pub fn handle_action(
    app: &mut App,
    action: Action,
    client: &Client,
    token: &mut auth::Token,
    auth_info: &auth::AuthInfo,
    tx: &mpsc::Sender<Event>,
) {
    match action {
        Action::TokenRefreshed(new_token) => {
            *token = new_token;
            app.status = "Token refreshed successfully!".to_string();
        }
        Action::LoadFiles(files) => {
            app.files = files;
            app.status = format!("Loaded {} items.", app.files.len());
            app.state
                .select(if app.files.is_empty() { None } else { Some(0) });
        }
        Action::LoadTrash(files) => {
            app.trashed_files = files;
            app.status = format!("Loaded {} trashed items.", app.trashed_files.len());
            app.trash_state.select(if app.trashed_files.is_empty() {
                None
            } else {
                Some(0)
            });
        }
        Action::Message(msg) => {
            app.status = msg;
        }
        Action::QueueDownloads(files) => {
            for f in files {
                app.dl_manager.queue.push(crate::app::DownloadTask {
                    file: f,
                    total_bytes: 0,
                    downloaded_bytes: 0,
                    status: crate::app::DownloadStatus::Pending,
                });
            }
            // Start if none active
            if app.active_dl_task.is_none() {
                if let Some(first) = app.dl_manager.queue.first_mut() {
                    first.status = crate::app::DownloadStatus::Downloading;
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    let f = first.file.clone();
                    app.active_dl_task = Some(tokio::spawn(async move {
                        crate::download::download_file_ranged(c, t, f, 0, txc).await;
                    }));
                }
            }
        }
        Action::UpdateDownloadProgress(id, dl, total, speed) => {
            if let Some(task) = app.dl_manager.queue.iter_mut().find(|t| t.file.id == id) {
                task.downloaded_bytes = dl;
                task.total_bytes = total;
            }
            app.dl_manager.speed_history.push_back(speed as u64);
            if app.dl_manager.speed_history.len() > 100 {
                app.dl_manager.speed_history.pop_front();
            }
            // Keep legacy global progress updated
            app.download_progress = Some((dl, total, speed));
        }
        Action::QueueUploads(tasks) => {
            for t in tasks {
                app.ul_manager.queue.push(t);
            }
            if app.active_ul_task.is_none() {
                if let Some(first) = app
                    .ul_manager
                    .queue
                    .iter_mut()
                    .find(|t| t.status == crate::app::UploadStatus::Pending)
                {
                    first.status = crate::app::UploadStatus::Uploading;
                    let task_clone = first.clone();
                    let client_c = client.clone();
                    let token_c = token.access_token.clone();
                    let tx_c = tx.clone();
                    app.active_ul_task = Some(tokio::spawn(async move {
                        upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                    }));
                }
            }
            app.status = format!("{} upload(s) queued.", app.ul_manager.queue.len());
        }
        Action::UpdateUploadProgress(id, downloaded, total, speed) => {
            if let Some(task) = app.ul_manager.queue.iter_mut().find(|t| t.local_path == id) {
                task.uploaded_bytes = downloaded;
                task.total_bytes = total;
            }
            app.ul_manager.speed_history.push_back(speed as u64);
            if app.ul_manager.speed_history.len() > 100 {
                app.ul_manager.speed_history.pop_front();
            }
            app.upload_progress = Some((downloaded, total, speed));
        }
        Action::CompleteUpload(id) => {
            app.ul_manager.queue.retain(|t| t.local_path != id);
            app.active_ul_task = None;
            app.upload_progress = None;

            if let Some(first) = app
                .ul_manager
                .queue
                .iter_mut()
                .find(|t| t.status == crate::app::UploadStatus::Pending)
            {
                first.status = crate::app::UploadStatus::Uploading;
                let task_clone = first.clone();
                let client_c = client.clone();
                let token_c = token.access_token.clone();
                let tx_c = tx.clone();
                app.active_ul_task = Some(tokio::spawn(async move {
                    upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                }));
            } else if !app.ul_manager.queue.is_empty()
                && app
                    .ul_manager
                    .queue
                    .iter()
                    .all(|t| t.status == crate::app::UploadStatus::Paused)
            {
                app.status = "Uploads paused.".into();
            } else if app.ul_manager.queue.is_empty() {
                app.status = "All uploads complete.".into();

                // Automatically fetch files after upload is done!
                let txc = tx.clone();
                let client_c = client.clone();
                let token_c = token.access_token.clone();
                let current = app.current_path.clone();
                tokio::spawn(async move {
                    let q = format!("'{}' in parents and trashed = false", current);
                    crate::api::fetch_files(client_c, token_c, q, txc).await;
                });
            }
        }
        Action::CompleteDownload(id) => {
            app.dl_manager.queue.retain(|t| t.file.id != id);
            app.active_dl_task = None;
            app.download_progress = None;

            // Start next pending
            if let Some(first) = app
                .dl_manager
                .queue
                .iter_mut()
                .find(|t| t.status == crate::app::DownloadStatus::Pending)
            {
                first.status = crate::app::DownloadStatus::Downloading;
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                let f = first.file.clone();
                let start_bytes = first.downloaded_bytes;
                app.active_dl_task = Some(tokio::spawn(async move {
                    crate::download::download_file_ranged(c, t, f, start_bytes, txc).await;
                }));
            } else if !app.dl_manager.queue.is_empty()
                && app
                    .dl_manager
                    .queue
                    .iter()
                    .all(|t| t.status == crate::app::DownloadStatus::Paused)
            {
                app.status = "Downloads paused.".into();
            } else if app.dl_manager.queue.is_empty() {
                app.status = "All downloads complete.".into();
            }
        }
        Action::Error(err) => {
            app.status = format!("Error: {}", err);
            app.download_progress = None;
            app.upload_progress = None;
            if err.contains("401 Unauthorized") {
                app.status = "Token expired! Refreshing automatically...".to_string();
                let client_c = client.clone();
                let mut token_c = token.clone();
                let auth_info_c = auth_info.clone();
                let tx_c = tx.clone();
                tokio::spawn(async move {
                    match crate::auth::refresh_token_if_needed(
                        &client_c,
                        &auth_info_c,
                        &mut token_c,
                    )
                    .await
                    {
                        Ok(_) => {
                            let _ = tx_c
                                .send(Event::Action(Action::TokenRefreshed(token_c)))
                                .await;
                        }
                        Err(e) => {
                            let _ = tx_c
                                .send(Event::Action(Action::Error(format!(
                                    "Refresh failed: {}",
                                    e
                                ))))
                                .await;
                        }
                    }
                });
            }
        }

        Action::LoadQuota(used, limit) => {
            app.storage_quota = Some((used, limit));
        }

        Action::UploadComplete(msg) => {
            app.upload_progress = None;
            app.status = msg;
        }
        Action::ImagePreview(bytes) => {
            if app.show_preview {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    app.preview_dims = Some((img.width(), img.height()));
                    app.preview_image = Some(app.picker.new_resize_protocol(img));
                }
            }
        }
    }
}

pub async fn handle_suspend_and_edit(
    app: &mut App,
    file: crate::app::DriveFile,
    client: &Client,
    token: &auth::Token,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> anyhow::Result<()> {
    // Suspend UI
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let tmp_path = format!("/tmp/doraivu_edit_{}", file.name);
    println!("Downloading {} for editing...", file.name);

    // Blocking download
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?alt=media",
        file.id
    );
    match client
        .get(&url)
        .bearer_auth(&token.access_token)
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            let bytes = res.bytes().await?;
            std::fs::write(&tmp_path, &bytes)?;

            let before_metadata = std::fs::metadata(&tmp_path)?;

            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
            let mut child = tokio::process::Command::new(editor)
                .arg(&tmp_path)
                .spawn()?;

            child.wait().await?;

            let after_metadata = std::fs::metadata(&tmp_path)?;
            if after_metadata.modified()? > before_metadata.modified()? {
                println!("File changed, uploading...");

                let new_content = std::fs::read(&tmp_path)?;
                let upload_url = format!(
                    "https://www.googleapis.com/upload/drive/v3/files/{}?uploadType=media",
                    file.id
                );
                let patch_res = client
                    .patch(&upload_url)
                    .bearer_auth(&token.access_token)
                    .header("Content-Type", &file.mime_type)
                    .body(new_content)
                    .send()
                    .await?;

                if patch_res.status().is_success() {
                    app.status = format!("Successfully updated {}", file.name);
                } else {
                    app.status = format!("Upload failed: {}", patch_res.status());
                }
            } else {
                app.status = "No changes made.".into();
            }
            let _ = std::fs::remove_file(&tmp_path);
        }
        _ => {
            app.status = "Failed to download file for editing.".into();
        }
    }

    Ok(())
}
