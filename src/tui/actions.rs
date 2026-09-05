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
            let is_dl_running = app
                .download
                .active_task
                .as_ref()
                .is_some_and(|h| !h.is_finished());
            if !is_dl_running {
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
                    if let Some(h) = app.download.active_task.take() {
                        h.abort();
                    }
                    app.download.active_task = Some(tokio::spawn(async move {
                        crate::drive::download::download_file_ranged(c, t, f, start_bytes, txc)
                            .await;
                    }));
                }
            }

            // 2. Resume active or pending upload automatically
            let is_ul_running = app
                .upload
                .active_task
                .as_ref()
                .is_some_and(|h| !h.is_finished());
            if !is_ul_running {
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
                    if let Some(h) = app.upload.active_task.take() {
                        h.abort();
                    }
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
        Action::LoadFiles(mut files) => {
            crate::drive::api::sort_files(&mut files);
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
        Action::LoadTrash(mut files) => {
            crate::drive::api::sort_files(&mut files);
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
            let is_dl_running = app
                .download
                .active_task
                .as_ref()
                .is_some_and(|h| !h.is_finished());
            if !is_dl_running {
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
                    if let Some(h) = app.download.active_task.take() {
                        h.abort();
                    }
                    app.download.active_task = Some(tokio::spawn(async move {
                        crate::drive::download::download_file_ranged(c, t, f, start_bytes, txc)
                            .await;
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
                task.status = models::DownloadStatus::Downloading;
            }
            if app.status.contains("reconnecting") || app.status.contains("Connection lost") {
                if let Some(task) = app.download.manager.queue.iter().find(|t| t.file.id == id) {
                    app.status = format!("Downloading {}...", task.file.name);
                } else {
                    app.status = "Downloading...".to_string();
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
            let is_ul_running = app
                .upload
                .active_task
                .as_ref()
                .is_some_and(|h| !h.is_finished());
            if !is_ul_running {
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
                    if let Some(h) = app.upload.active_task.take() {
                        h.abort();
                    }
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
                task.status = models::UploadStatus::Uploading;
            }
            if app.status.contains("reconnecting") || app.status.contains("Connection lost") {
                if let Some(task) = app.upload.manager.queue.iter().find(|t| t.local_path == id) {
                    app.status = format!("Uploading {}...", task.name);
                } else {
                    app.status = "Uploading...".to_string();
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
                app.status = format!("Uploading {}...", task_clone.name);
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
                app.status = format!("Downloading {}...", f.name);
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
                if let Some(h) = app.download.active_task.take() {
                    h.abort();
                }
                if let Some(h) = app.upload.active_task.take() {
                    h.abort();
                }

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
            } else {
                if let Some(ref h) = app.download.active_task {
                    if h.is_finished() {
                        app.download.active_task = None;
                    }
                }
                if let Some(ref h) = app.upload.active_task {
                    if h.is_finished() {
                        app.upload.active_task = None;
                    }
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
        Action::PreviewMetadataLoaded {
            id,
            name,
            size,
            items_count,
            is_calculating,
            created,
            modified,
        } => {
            if app.preview.mode != PreviewMode::Hidden {
                let is_current = app.selected_file().is_some_and(|f| f.id == id);
                if is_current {
                    app.preview.state = PreviewState::Metadata {
                        name,
                        size,
                        items_count,
                        is_calculating,
                        created,
                        modified,
                    };
                }
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
        Action::SetDownloadDownloading(id) => {
            if let Some(task) = app
                .download
                .manager
                .queue
                .iter_mut()
                .find(|t| t.file.id == id)
            {
                task.status = models::DownloadStatus::Downloading;
                if app.status.contains("reconnecting") || app.status.contains("Connection lost") {
                    app.status = format!("Downloading {}...", task.file.name);
                }
            }
        }
        Action::SetUploadUploading(local_path) => {
            if let Some(task) = app
                .upload
                .manager
                .queue
                .iter_mut()
                .find(|t| t.local_path == local_path)
            {
                task.status = models::UploadStatus::Uploading;
                if app.status.contains("reconnecting") || app.status.contains("Connection lost") {
                    app.status = format!("Uploading {}...", task.name);
                }
            }
        }
        Action::DownloadFailed(id, msg) => {
            if let Some(task) = app
                .download
                .manager
                .queue
                .iter_mut()
                .find(|t| t.file.id == id)
            {
                task.status = models::DownloadStatus::Paused;
            }
            if let Some(h) = app.download.active_task.take() {
                h.abort();
            }
            app.download.progress = None;
            app.status = format!("Download paused ({}). Press 'r' in tracker to retry.", msg);
        }
        Action::UploadFailed(local_path, msg) => {
            if let Some(task) = app
                .upload
                .manager
                .queue
                .iter_mut()
                .find(|t| t.local_path == local_path)
            {
                task.status = models::UploadStatus::Paused;
            }
            if let Some(h) = app.upload.active_task.take() {
                h.abort();
            }
            app.upload.progress = None;
            app.status = format!("Upload paused ({}). Press 'r' in tracker to retry.", msg);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_failed_pauses_task_and_cleans_up() {
        let mut app = App::new();
        let file = models::DriveFile {
            id: "file123".to_string(),
            name: "test.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            app_properties: None,
        };
        app.download.manager.queue.push(models::DownloadTask {
            file,
            total_bytes: 1024,
            downloaded_bytes: 512,
            status: models::DownloadStatus::Reconnecting,
        });
        app.download.progress = Some((512, 1024, 100.0));

        let client = Client::new();
        let mut token = auth::Token {
            access_token: "token".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
            refresh_token: None,
            scope: "drive".to_string(),
        };
        let auth_info = auth::AuthInfo {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        };
        let (tx, _rx) = mpsc::channel(1);

        handle_action(
            &mut app,
            Action::DownloadFailed("file123".to_string(), "Network unreachable".to_string()),
            &client,
            &mut token,
            &auth_info,
            &tx,
        );

        assert_eq!(
            app.download.manager.queue[0].status,
            models::DownloadStatus::Paused
        );
        assert!(app.download.active_task.is_none());
        assert!(app.download.progress.is_none());
        assert!(app.status.contains("Download paused"));
    }

    #[test]
    fn test_upload_failed_pauses_task_and_cleans_up() {
        let mut app = App::new();
        app.upload.manager.queue.push(models::UploadTask {
            local_path: "/tmp/upload.pdf".to_string(),
            name: "upload.pdf".to_string(),
            target_parent_id: "root".to_string(),
            total_bytes: 2048,
            uploaded_bytes: 1024,
            status: models::UploadStatus::Reconnecting,
        });
        app.upload.progress = Some((1024, 2048, 50.0));

        let client = Client::new();
        let mut token = auth::Token {
            access_token: "token".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
            refresh_token: None,
            scope: "drive".to_string(),
        };
        let auth_info = auth::AuthInfo {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        };
        let (tx, _rx) = mpsc::channel(1);

        handle_action(
            &mut app,
            Action::UploadFailed("/tmp/upload.pdf".to_string(), "Timeout".to_string()),
            &client,
            &mut token,
            &auth_info,
            &tx,
        );

        assert_eq!(
            app.upload.manager.queue[0].status,
            models::UploadStatus::Paused
        );
        assert!(app.upload.active_task.is_none());
        assert!(app.upload.progress.is_none());
        assert!(app.status.contains("Upload paused"));
    }

    #[test]
    fn test_set_download_downloading_clears_reconnecting_status() {
        let mut app = App::new();
        let file = models::DriveFile {
            id: "file123".to_string(),
            name: "video.mp4".to_string(),
            mime_type: "video/mp4".to_string(),
            app_properties: None,
        };
        app.download.manager.queue.push(models::DownloadTask {
            file,
            total_bytes: 5000,
            downloaded_bytes: 2500,
            status: models::DownloadStatus::Reconnecting,
        });
        app.status = "Connection lost, reconnecting (attempt 3)...".to_string();

        let client = Client::new();
        let mut token = auth::Token {
            access_token: "token".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
            refresh_token: None,
            scope: "drive".to_string(),
        };
        let auth_info = auth::AuthInfo {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        };
        let (tx, _rx) = mpsc::channel(1);

        handle_action(
            &mut app,
            Action::SetDownloadDownloading("file123".to_string()),
            &client,
            &mut token,
            &auth_info,
            &tx,
        );

        assert_eq!(
            app.download.manager.queue[0].status,
            models::DownloadStatus::Downloading
        );
        assert_eq!(app.status, "Downloading video.mp4...");
    }

    #[test]
    fn test_set_upload_uploading_clears_reconnecting_status() {
        let mut app = App::new();
        app.upload.manager.queue.push(models::UploadTask {
            local_path: "/tmp/data.csv".to_string(),
            name: "data.csv".to_string(),
            target_parent_id: "root".to_string(),
            total_bytes: 2048,
            uploaded_bytes: 1024,
            status: models::UploadStatus::Reconnecting,
        });
        app.status = "Connection lost, reconnecting (attempt 2)...".to_string();

        let client = Client::new();
        let mut token = auth::Token {
            access_token: "token".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
            refresh_token: None,
            scope: "drive".to_string(),
        };
        let auth_info = auth::AuthInfo {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        };
        let (tx, _rx) = mpsc::channel(1);

        handle_action(
            &mut app,
            Action::SetUploadUploading("/tmp/data.csv".to_string()),
            &client,
            &mut token,
            &auth_info,
            &tx,
        );

        assert_eq!(
            app.upload.manager.queue[0].status,
            models::UploadStatus::Uploading
        );
        assert_eq!(app.status, "Uploading data.csv...");
    }

    #[test]
    fn test_update_progress_clears_reconnecting_status() {
        let mut app = App::new();
        let file = models::DriveFile {
            id: "file999".to_string(),
            name: "doc.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            app_properties: None,
        };
        app.download.manager.queue.push(models::DownloadTask {
            file,
            total_bytes: 1000,
            downloaded_bytes: 200,
            status: models::DownloadStatus::Reconnecting,
        });
        app.status = "Connection lost, reconnecting (attempt 1)...".to_string();

        let client = Client::new();
        let mut token = auth::Token {
            access_token: "token".to_string(),
            expires_in: 3600,
            token_type: "Bearer".to_string(),
            refresh_token: None,
            scope: "drive".to_string(),
        };
        let auth_info = auth::AuthInfo {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
        };
        let (tx, _rx) = mpsc::channel(1);

        handle_action(
            &mut app,
            Action::UpdateDownloadProgress("file999".to_string(), 400, 1000, 200.0),
            &client,
            &mut token,
            &auth_info,
            &tx,
        );

        assert_eq!(
            app.download.manager.queue[0].status,
            models::DownloadStatus::Downloading
        );
        assert_eq!(app.status, "Downloading doc.pdf...");
    }
}
