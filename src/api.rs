use crate::app::{Action, DriveFile, Event};
use reqwest::Client;
use tokio::sync::mpsc;

/// Fetches files from Google Drive matching the query
pub async fn fetch_files(
    client: Client,
    access_token: String,
    query: String,
    tx: mpsc::Sender<Event>,
) {
    let url = format!(
        "https://www.googleapis.com/drive/v3/files?q={}&fields=files(id,name,mimeType)",
        urlencoding::encode(&query)
    );

    match client.get(&url).bearer_auth(&access_token).send().await {
        Ok(res) => {
            if res.status().is_success() {
                #[derive(serde::Deserialize)]
                struct FilesList {
                    files: Vec<DriveFile>,
                }
                match res.json::<FilesList>().await {
                    Ok(list) => {
                        let _ = tx.send(Event::Action(Action::LoadFiles(list.files))).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Event::Action(Action::Error(format!("JSON Parse: {}", e))))
                            .await;
                    }
                }
            } else {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                let _ = tx
                    .send(Event::Action(Action::Error(format!(
                        "API Error {}: {}",
                        status, body
                    ))))
                    .await;
            }
        }
        Err(e) => {
            let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
        }
    }
}

/// Moves a file to the trash in Google Drive
pub async fn trash_file(
    client: Client,
    access_token: String,
    file_id: String,
    tx: mpsc::Sender<Event>,
) {
    let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
    match client
        .patch(&url)
        .bearer_auth(&access_token)
        .json(&serde_json::json!({"trashed": true}))
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            let _ = tx
                .send(Event::Action(Action::Message(
                    "File moved to trash.".into(),
                )))
                .await;
        }
        Ok(res) => {
            let _ = tx
                .send(Event::Action(Action::Error(format!(
                    "Trash failed: {}",
                    res.status()
                ))))
                .await;
        }
        Err(e) => {
            let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
        }
    }
}

/// Downloads a file from Google Drive to the user's Downloads folder
pub async fn fetch_quota(client: Client, access_token: String, tx: mpsc::Sender<Event>) {
    let url = "https://www.googleapis.com/drive/v3/about?fields=storageQuota";
    match client.get(url).bearer_auth(&access_token).send().await {
        Ok(res) if res.status().is_success() => {
            #[derive(serde::Deserialize)]
            #[allow(non_snake_case)]
            struct QuotaResponse {
                storageQuota: StorageQuota,
            }
            #[derive(serde::Deserialize)]
            #[allow(non_snake_case)]
            struct StorageQuota {
                usage: Option<String>,
                limit: Option<String>,
            }

            if let Ok(data) = res.json::<QuotaResponse>().await {
                if let (Some(u), Some(l)) = (data.storageQuota.usage, data.storageQuota.limit) {
                    if let (Ok(used), Ok(limit)) = (u.parse::<u64>(), l.parse::<u64>()) {
                        let _ = tx.send(Event::Action(Action::LoadQuota(used, limit))).await;
                    }
                }
            }
        }
        _ => {} // Ignore errors for quota fetch silently
    }
}

/// Fetches image bytes for inline preview
pub async fn fetch_preview(
    client: Client,
    access_token: String,
    file_id: String,
    tx: mpsc::Sender<Event>,
) {
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?alt=media",
        file_id
    );
    match client.get(&url).bearer_auth(&access_token).send().await {
        Ok(res) if res.status().is_success() => {
            if let Ok(bytes) = res.bytes().await {
                let _ = tx
                    .send(Event::Action(Action::ImagePreview(bytes.to_vec())))
                    .await;
            }
        }
        _ => {}
    }
}

/// Uploads a file using multipart

async fn resolve_or_create_path(
    client: &Client,
    access_token: &str,
    path: &str,
    tx: &mpsc::Sender<Event>,
) -> anyhow::Result<String> {
    if !path.starts_with('/') {
        return Ok(path.to_string());
    }

    let mut current_id = "root".to_string();
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    for part in parts {
        let _ = tx
            .send(Event::Action(Action::UploadComplete(format!(
                "Resolving {}...",
                part
            ))))
            .await;

        let query = format!("mimeType = 'application/vnd.google-apps.folder' and name = '{}' and '{}' in parents and trashed = false", part, current_id);
        let url = format!(
            "https://www.googleapis.com/drive/v3/files?q={}&fields=files(id)",
            urlencoding::encode(&query)
        );

        let res = client.get(&url).bearer_auth(access_token).send().await?;
        if !res.status().is_success() {
            anyhow::bail!("API err {}", part);
        }

        let data: serde_json::Value = res.json().await?;
        if let Some(files) = data["files"].as_array() {
            if let Some(first) = files.first() {
                if let Some(id) = first["id"].as_str() {
                    current_id = id.to_string();
                    continue;
                }
            }
        }

        let _ = tx
            .send(Event::Action(Action::UploadComplete(format!(
                "Creating {}...",
                part
            ))))
            .await;

        let metadata = serde_json::json!({
            "name": part,
            "mimeType": "application/vnd.google-apps.folder",
            "parents": [current_id]
        });

        let res = client
            .post("https://www.googleapis.com/drive/v3/files")
            .bearer_auth(access_token)
            .json(&metadata)
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!("Fail create {}", part);
        }

        let data: serde_json::Value = res.json().await?;
        if let Some(id) = data["id"].as_str() {
            current_id = id.to_string();
        } else {
            anyhow::bail!("No ID {}", part);
        }
    }

    Ok(current_id)
}

pub async fn upload_file(
    client: Client,
    access_token: String,
    parent_path_or_id: String,
    local_path: String,
    tx: mpsc::Sender<Event>,
) {
    let parent_id =
        match resolve_or_create_path(&client, &access_token, &parent_path_or_id, &tx).await {
            Ok(id) => id,
            Err(e) => {
                let _ = tx
                    .send(Event::Action(Action::Error(format!("Path err: {}", e))))
                    .await;
                return;
            }
        };

    let metadata = match tokio::fs::metadata(&local_path).await {
        Ok(m) => m,
        Err(e) => {
            let _ = tx
                .send(Event::Action(Action::Error(format!("Metadata err: {}", e))))
                .await;
            return;
        }
    };

    if metadata.is_dir() {
        let mut queue = vec![(std::path::PathBuf::from(&local_path), parent_id)];
        let _count = 0;

        while let Some((current_dir, current_parent_id)) = queue.pop() {
            let mut entries = match tokio::fs::read_dir(&current_dir).await {
                Ok(e) => e,
                Err(_) => continue,
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let entry_meta = match entry.metadata().await {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                if entry_meta.is_dir() {
                    let _ = tx
                        .send(Event::Action(Action::UploadComplete(format!(
                            "Creating folder {}...",
                            name
                        ))))
                        .await;

                    let query = format!("mimeType = 'application/vnd.google-apps.folder' and name = '{}' and '{}' in parents and trashed = false", name, current_parent_id);
                    let url = format!(
                        "https://www.googleapis.com/drive/v3/files?q={}&fields=files(id)",
                        urlencoding::encode(&query)
                    );

                    let res = client.get(&url).bearer_auth(&access_token).send().await;
                    let mut new_id = None;
                    if let Ok(r) = res {
                        if r.status().is_success() {
                            if let Ok(data) = r.json::<serde_json::Value>().await {
                                if let Some(files) = data["files"].as_array() {
                                    if let Some(first) = files.first() {
                                        if let Some(id) = first["id"].as_str() {
                                            new_id = Some(id.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let next_parent_id = if let Some(id) = new_id {
                        id
                    } else {
                        let meta = serde_json::json!({
                            "name": name,
                            "mimeType": "application/vnd.google-apps.folder",
                            "parents": [current_parent_id]
                        });
                        let res = client
                            .post("https://www.googleapis.com/drive/v3/files")
                            .bearer_auth(&access_token)
                            .json(&meta)
                            .send()
                            .await;

                        match res {
                            Ok(r) if r.status().is_success() => {
                                if let Ok(data) = r.json::<serde_json::Value>().await {
                                    data["id"].as_str().unwrap_or_default().to_string()
                                } else {
                                    continue;
                                }
                            }
                            _ => continue,
                        }
                    };
                    queue.push((path, next_parent_id));
                } else {
                    let total_bytes = entry_meta.len();
                    let task = crate::app::UploadTask {
                        local_path: path.to_string_lossy().to_string(),
                        name,
                        target_parent_id: current_parent_id.clone(),
                        total_bytes,
                        uploaded_bytes: 0,
                        status: crate::app::UploadStatus::Pending,
                    };
                    let _ = tx
                        .send(Event::Action(Action::QueueUploads(vec![task])))
                        .await;
                }
            }
        }
        let _ = tx
            .send(Event::Action(Action::UploadComplete(
                "Directory processed. Uploads queued.".into(),
            )))
            .await;
    } else {
        let file_name = std::path::Path::new(&local_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let total_bytes = metadata.len();
        let task = crate::app::UploadTask {
            local_path,
            name: file_name,
            target_parent_id: parent_id,
            total_bytes,
            uploaded_bytes: 0,
            status: crate::app::UploadStatus::Pending,
        };
        let _ = tx
            .send(Event::Action(Action::QueueUploads(vec![task])))
            .await;
    }
}
