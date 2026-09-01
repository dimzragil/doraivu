use reqwest::Client;
use tokio::sync::mpsc;
use crate::app::{Action, DriveFile, Event};

/// Fetches files from Google Drive matching the query
pub async fn fetch_files(client: Client, access_token: String, query: String, tx: mpsc::Sender<Event>) {
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
pub async fn download_file(
    client: Client,
    access_token: String,
    file: DriveFile,
    tx: mpsc::Sender<Event>,
) {
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?alt=media",
        file.id
    );
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dest_path = format!("{}/Downloads/{}", home, file.name);

    match client.get(&url).bearer_auth(&access_token).send().await {
        Ok(res) if res.status().is_success() => {
            let total_size = res.content_length().unwrap_or(0);
            let mut downloaded = 0;

            if let Ok(mut file_out) = tokio::fs::File::create(&dest_path).await {
                use futures_util::StreamExt;
                let mut stream = res.bytes_stream();
                let start_time = std::time::Instant::now();
                let mut last_update = std::time::Instant::now();
                
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            if tokio::io::AsyncWriteExt::write_all(&mut file_out, &chunk)
                                .await
                                .is_err()
                            {
                                let _ = tx
                                    .send(Event::Action(Action::Error(
                                        "Failed to write to file".into(),
                                    )))
                                    .await;
                                return;
                            }
                            downloaded += chunk.len() as u64;
                            
                            let now = std::time::Instant::now();
                            if now.duration_since(last_update).as_millis() > 100 {
                                let elapsed = now.duration_since(start_time).as_secs_f64();
                                let speed = if elapsed > 0.0 {
                                    downloaded as f64 / elapsed
                                } else {
                                    0.0
                                };
                                let _ = tx
                                    .send(Event::Action(Action::DownloadProgress(
                                        downloaded, total_size, speed
                                    )))
                                    .await;
                                last_update = now;
                            }
                        }
                        Err(_) => {
                            let _ = tx
                                .send(Event::Action(Action::Error("Download stream error".into())))
                                .await;
                            return;
                        }
                    }
                }
                let _ = tx
                    .send(Event::Action(Action::DownloadComplete(format!(
                        "Saved to {}",
                        dest_path
                    ))))
                    .await;
            } else {
                let _ = tx
                    .send(Event::Action(Action::Error(
                        "Could not create local file".into(),
                    )))
                    .await;
            }
        }
        Ok(res) => {
            let _ = tx
                .send(Event::Action(Action::Error(format!(
                    "Download failed: {}",
                    res.status()
                ))))
                .await;
        }
        Err(e) => {
            let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
        }
    }
}

/// Fetches storage quota from Google Drive API
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
    let url = format!("https://www.googleapis.com/drive/v3/files/{}?alt=media", file_id);
    match client.get(&url).bearer_auth(&access_token).send().await {
        Ok(res) if res.status().is_success() => {
            if let Ok(bytes) = res.bytes().await {
                let _ = tx.send(Event::Action(Action::ImagePreview(bytes.to_vec()))).await;
            }
        }
        _ => {}
    }
}
