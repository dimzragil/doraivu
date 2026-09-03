use crate::drive::{auth, models, upload};
use crate::tui::state::{Action, App, Event, PreviewMode, PreviewState};
use reqwest::Client;
use tokio::sync::mpsc;

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
                app.dl_manager.queue.push(models::DownloadTask {
                    file: f,
                    total_bytes: 0,
                    downloaded_bytes: 0,
                    status: models::DownloadStatus::Pending,
                });
            }
            // Start if none active
            if app.active_dl_task.is_none() {
                if let Some(first) = app.dl_manager.queue.first_mut() {
                    first.status = models::DownloadStatus::Downloading;
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    let f = first.file.clone();
                    app.active_dl_task = Some(tokio::spawn(async move {
                        crate::drive::download::download_file_ranged(c, t, f, 0, txc).await;
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
            } else if !app.ul_manager.queue.is_empty()
                && app
                    .ul_manager
                    .queue
                    .iter()
                    .all(|t| t.status == models::UploadStatus::Paused)
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
                    if current != "virtual_root" {
                        let q = if current == "shared_with_me" {
                            "sharedWithMe = true and trashed = false".to_string()
                        } else {
                            format!("'{}' in parents and trashed = false", current)
                        };
                        crate::drive::api::fetch_files(client_c, token_c, q, txc).await;
                    }
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
                .find(|t| t.status == models::DownloadStatus::Pending)
            {
                first.status = models::DownloadStatus::Downloading;
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                let f = first.file.clone();
                let start_bytes = first.downloaded_bytes;
                app.active_dl_task = Some(tokio::spawn(async move {
                    crate::drive::download::download_file_ranged(c, t, f, start_bytes, txc).await;
                }));
            } else if !app.dl_manager.queue.is_empty()
                && app
                    .dl_manager
                    .queue
                    .iter()
                    .all(|t| t.status == models::DownloadStatus::Paused)
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
                    match auth::refresh_token_if_needed(&client_c, &auth_info_c, &mut token_c).await
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
            if app.preview_mode != PreviewMode::Hidden {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    app.preview_dims = Some((img.width(), img.height()));
                    app.preview_state = PreviewState::Image(app.picker.new_resize_protocol(img));
                }
            }
        }
        Action::PreviewMetadataLoaded(name, size, created, modified) => {
            if app.preview_mode != PreviewMode::Hidden {
                app.preview_state = PreviewState::Metadata {
                    name,
                    size,
                    created,
                    modified,
                };
            }
        }
    }
}
