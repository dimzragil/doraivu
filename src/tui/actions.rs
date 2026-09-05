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
            app.token_refreshed_at = std::time::Instant::now();
            app.is_refreshing_token = false;
            if app.download.active_task.is_none() && app.upload.active_task.is_none() {
                app.status = "Token refreshed successfully!".to_string();
            }

            // 1. Resume active or pending download automatically
            if app.download.active_task.is_none() {
                if let Some(target) = app
                    .download
                    .manager
                    .queue
                    .iter_mut()
                    .find(|t| t.status != models::DownloadStatus::Paused)
                {
                    target.status = models::DownloadStatus::Downloading;
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    let f = target.file.clone();
                    let start_bytes = target.downloaded_bytes;
                    app.status = format!("Resuming download for {}...", f.name);
                    app.download.active_task = Some(tokio::spawn(async move {
                        crate::drive::download::download_file_ranged(c, t, f, start_bytes, txc)
                            .await;
                    }));
                }
            }

            // 2. Resume active or pending upload automatically
            if app.upload.active_task.is_none() {
                if let Some(target) = app
                    .upload
                    .manager
                    .queue
                    .iter_mut()
                    .find(|t| t.status != models::UploadStatus::Paused)
                {
                    target.status = models::UploadStatus::Uploading;
                    let task_clone = target.clone();
                    let client_c = client.clone();
                    let token_c = token.access_token.clone();
                    let tx_c = tx.clone();
                    app.status = format!("Resuming upload for {}...", target.name);
                    app.upload.active_task = Some(tokio::spawn(async move {
                        upload::upload_file_task(client_c, token_c, task_clone, tx_c).await;
                    }));
                }
            }

            // 3. Refresh file view if currently in a real folder
            if app.nav.current_path != "virtual_root" {
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                let parent_id = app.nav.current_path.clone();
                tokio::spawn(async move {
                    let q = if parent_id == "shared_with_me" {
                        "sharedWithMe = true and trashed = false".to_string()
                    } else {
                        format!("'{}' in parents and trashed = false", parent_id)
                    };
                    crate::drive::api::fetch_files(c, t, q, txc).await;
                });
            }
        }
        Action::TokenRefreshFailed => {
            app.is_refreshing_token = false;
        }
        Action::LoadFiles(files) => {
            app.files = files;
            app.status = format!("Loaded {} items.", app.files.len());
            app.state
                .select(if app.files.is_empty() { None } else { Some(0) });
        }
        Action::RenameSuccess => {
            app.status = "Rename succeeded. Refreshing...".to_string();
            let current_path = app.nav.current_path.clone();

            if current_path == "virtual_root" {
                app.files = models::DriveFile::virtual_root_items();
                app.state.select(Some(0));
                return;
            }

            let c = client.clone();
            let t = token.access_token.clone();
            let txc = tx.clone();
            tokio::spawn(async move {
                let query = if current_path == "shared_with_me" {
                    "sharedWithMe = true and trashed = false".to_string()
                } else {
                    format!("'{}' in parents and trashed = false", current_path)
                };
                crate::drive::api::fetch_files(c, t, query, txc).await;
            });
        }
        Action::LoadTrash(files) => {
            app.trash.files = files;
            app.status = format!("Loaded {} trashed items.", app.trash.files.len());
            app.trash.state.select(if app.trash.files.is_empty() {
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
                app.download.manager.queue.push(models::DownloadTask {
                    file: f,
                    total_bytes: 0,
                    downloaded_bytes: 0,
                    status: models::DownloadStatus::Pending,
                });
            }
            // Start if none active
            if app.download.active_task.is_none() {
                if let Some(first) = app.download.manager.queue.first_mut() {
                    first.status = models::DownloadStatus::Downloading;
                    let c = client.clone();
                    let t = token.access_token.clone();
                    let txc = tx.clone();
                    let f = first.file.clone();
                    app.download.active_task = Some(tokio::spawn(async move {
                        crate::drive::download::download_file_ranged(c, t, f, 0, txc).await;
                    }));
                }
            }
        }
        Action::UpdateDownloadProgress(id, dl, total, speed) => {
            if let Some(task) = app
                .download
                .manager
                .queue
                .iter_mut()
                .find(|t| t.file.id == id)
            {
                task.downloaded_bytes = dl;
                task.total_bytes = total;
                if task.status == models::DownloadStatus::Reconnecting {
                    task.status = models::DownloadStatus::Downloading;
                }
            }
            app.download.manager.speed_history.push_back(speed as u64);
            if app.download.manager.speed_history.len() > 100 {
                app.download.manager.speed_history.pop_front();
            }
            app.download.progress = Some((dl, total, speed));
        }
        Action::QueueUploads(tasks) => {
            for t in tasks {
                app.upload.manager.queue.push(t);
            }
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
            app.status = format!("{} upload(s) queued.", app.upload.manager.queue.len());
        }
        Action::UpdateUploadProgress(id, downloaded, total, speed) => {
            if let Some(task) = app
                .upload
                .manager
                .queue
                .iter_mut()
                .find(|t| t.local_path == id)
            {
                task.uploaded_bytes = downloaded;
                task.total_bytes = total;
                if task.status == models::UploadStatus::Reconnecting {
                    task.status = models::UploadStatus::Uploading;
                }
            }
            app.upload.manager.speed_history.push_back(speed as u64);
            if app.upload.manager.speed_history.len() > 100 {
                app.upload.manager.speed_history.pop_front();
            }
            app.upload.progress = Some((downloaded, total, speed));
        }
        Action::CompleteUpload(id) => {
            app.upload.manager.queue.retain(|t| t.local_path != id);
            app.upload.active_task = None;
            app.upload.progress = None;

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
            } else if !app.upload.manager.queue.is_empty()
                && app
                    .upload
                    .manager
                    .queue
                    .iter()
                    .all(|t| t.status == models::UploadStatus::Paused)
            {
                app.status = "Uploads paused.".into();
            } else if app.upload.manager.queue.is_empty() {
                app.status = "All uploads complete.".into();

                // Automatically fetch files after upload is done!
                let txc = tx.clone();
                let client_c = client.clone();
                let token_c = token.access_token.clone();
                let current = app.nav.current_path.clone();
                tokio::spawn(async move {
                    if current != "virtual_root" {
                        let q = if current == "shared_with_me" {
                            "sharedWithMe = true and trashed = false".to_string()
                        } else {
                            format!("'{}' in parents and trashed = false", current)
                        };
                        crate::drive::api::fetch_files(
                            client_c.clone(),
                            token_c.clone(),
                            q,
                            txc.clone(),
                        )
                        .await;
                        crate::drive::api::fetch_quota(client_c, token_c, txc).await;
                    }
                });
            }
        }
        Action::CompleteDownload(id) => {
            app.download.manager.queue.retain(|t| t.file.id != id);
            app.download.active_task = None;
            app.download.progress = None;

            // Start next pending
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
                    crate::drive::download::download_file_ranged(c, t, f, start_bytes, txc).await;
                }));
            } else if !app.download.manager.queue.is_empty()
                && app
                    .download
                    .manager
                    .queue
                    .iter()
                    .all(|t| t.status == models::DownloadStatus::Paused)
            {
                app.status = "Downloads paused.".into();
            } else if app.download.manager.queue.is_empty() {
                app.status = "All downloads complete.".into();
            }
        }
        Action::Error(err) => {
            app.status = format!("Error: {}", err);
            let lower_err = err.to_lowercase();
            if lower_err.contains("401")
                || lower_err.contains("unauthorized")
                || lower_err.contains("invalid credentials")
                || lower_err.contains("unauthenticated")
            {
                app.download.progress = None;
                app.upload.progress = None;
                app.download.active_task = None;
                app.upload.active_task = None;

                if !app.is_refreshing_token {
                    app.is_refreshing_token = true;
                    app.status =
                        "Token expired! Refreshing token and resuming automatically...".to_string();
                    let client_c = client.clone();
                    let mut token_c = token.clone();
                    let auth_info_c = auth_info.clone();
                    let tx_c = tx.clone();
                    tokio::spawn(async move {
                        let mut retries = 0;
                        loop {
                            match auth::refresh_token_if_needed(
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
                                    break;
                                }
                                Err(e) => {
                                    retries += 1;
                                    if retries >= 5 {
                                        let _ = tx_c
                                            .send(Event::Action(Action::Error(format!(
                                                "Token refresh failed after retries: {}",
                                                e
                                            ))))
                                            .await;
                                        let _ = tx_c
                                            .send(Event::Action(Action::TokenRefreshFailed))
                                            .await;
                                        break;
                                    }
                                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                }
                            }
                        }
                    });
                }
            }
        }

        Action::LoadQuota(used, limit) => {
            app.storage_quota = Some((used, limit));
        }

        Action::UploadComplete(msg) => {
            app.upload.progress = None;
            app.status = msg;
        }
        Action::ImagePreview(bytes) => {
            if app.preview.mode != PreviewMode::Hidden {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    app.preview.dims = Some((img.width(), img.height()));
                    app.preview.state =
                        PreviewState::Image(app.preview.picker.new_resize_protocol(img));
                }
            }
        }
        Action::PreviewMetadataLoaded(name, size, created, modified) => {
            if app.preview.mode != PreviewMode::Hidden {
                app.preview.state = PreviewState::Metadata {
                    name,
                    size,
                    created,
                    modified,
                };
            }
        }
        Action::UpdateResumeTime(id, time) => {
            if let Some(f) = app.files.iter_mut().find(|f| f.id == id) {
                let props = f
                    .app_properties
                    .get_or_insert_with(std::collections::HashMap::new);
                props.insert("mpv_resume_time".to_string(), time.clone());
            }
            app.status = format!("Saved playback position ({}s).", time);
        }
        Action::SetDownloadReconnecting(id) => {
            if let Some(task) = app
                .download
                .manager
                .queue
                .iter_mut()
                .find(|t| t.file.id == id)
            {
                task.status = models::DownloadStatus::Reconnecting;
            }
        }
        Action::SetUploadReconnecting(local_path) => {
            if let Some(task) = app
                .upload
                .manager
                .queue
                .iter_mut()
                .find(|t| t.local_path == local_path)
            {
                task.status = models::UploadStatus::Reconnecting;
            }
        }
        Action::RefreshFolder(folder_id) => {
            if app.nav.current_path == folder_id {
                let c = client.clone();
                let t = token.access_token.clone();
                let txc = tx.clone();
                tokio::spawn(async move {
                    let q = if folder_id == "shared_with_me" {
                        "sharedWithMe = true and trashed = false".to_string()
                    } else {
                        format!("'{}' in parents and trashed = false", folder_id)
                    };
                    crate::drive::api::fetch_files(c.clone(), t.clone(), q, txc.clone()).await;
                    crate::drive::api::fetch_quota(c, t, txc).await;
                });
            }
        }
        Action::ClearClipboard => {
            app.clipboard = None;
        }
    }
}
