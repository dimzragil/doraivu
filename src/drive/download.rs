use crate::drive::models::DriveFile;
use crate::tui::state::{Action, Event};
use reqwest::Client;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

pub async fn download_file_ranged(
    client: Client,
    access_token: String,
    file: DriveFile,
    start_bytes: u64,
    tx: mpsc::Sender<Event>,
) {
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?alt=media",
        file.id
    );

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let downloads_dir = format!("{}/Downloads", home);
    let _ = tokio::fs::create_dir_all(&downloads_dir).await;
    let dest_path = format!("{}/{}", downloads_dir, file.name);

    let mut total_size: Option<u64> = None;
    let mut attempt = 0;
    let backoff_delays = [2, 3, 5, 10];
    let max_retries = 60;

    // If starting from 0 initially, truncate any existing file
    if start_bytes == 0 {
        let _ = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&dest_path)
            .await;
    }

    loop {
        // Dynamically inspect local file size to resume from exact byte
        let current_bytes = match tokio::fs::metadata(&dest_path).await {
            Ok(meta) => meta.len(),
            Err(_) => start_bytes,
        };

        if let Some(total) = total_size {
            if total > 0 && current_bytes >= total {
                let _ = tx
                    .send(Event::Action(Action::CompleteDownload(file.id)))
                    .await;
                return;
            }
        }

        let mut req = client.get(&url).bearer_auth(&access_token);
        if current_bytes > 0 {
            req = req.header("Range", format!("bytes={}-", current_bytes));
        }

        match req.send().await {
            Ok(res) => {
                let status = res.status();
                if status.as_u16() == 401 {
                    let _ = tx
                        .send(Event::Action(Action::Error("401 Unauthorized".to_string())))
                        .await;
                    return;
                }

                if status.as_u16() == 416 {
                    // Range Not Satisfiable -> file is already fully downloaded
                    let _ = tx
                        .send(Event::Action(Action::CompleteDownload(file.id)))
                        .await;
                    return;
                }

                if (status.is_server_error() || status.as_u16() == 429) && attempt < max_retries {
                    let delay = if attempt < backoff_delays.len() {
                        backoff_delays[attempt]
                    } else {
                        10
                    };
                    attempt += 1;
                    let _ = tx
                        .send(Event::Action(Action::SetDownloadReconnecting(
                            file.id.clone(),
                        )))
                        .await;
                    let _ = tx
                        .send(Event::Action(Action::Message(format!(
                            "Connection lost, reconnecting (attempt {})...",
                            attempt
                        ))))
                        .await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    continue;
                }

                if !status.is_success() && status.as_u16() != 206 {
                    let body = res.text().await.unwrap_or_default();
                    let _ = tx
                        .send(Event::Action(Action::DownloadFailed(
                            file.id.clone(),
                            format!("DL failed ({}): {}", status, body),
                        )))
                        .await;
                    return;
                }

                let reported_len = res.content_length().unwrap_or(0);
                let current_total = if status.as_u16() == 206 {
                    reported_len.saturating_add(current_bytes)
                } else if current_bytes == 0 {
                    reported_len
                } else {
                    // Server sent entire file from 0; truncate and restart
                    let _ = tokio::fs::write(&dest_path, &[]).await;
                    reported_len
                };
                total_size = Some(current_total);

                let mut file_out = match tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&dest_path)
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = tx
                            .send(Event::Action(Action::DownloadFailed(
                                file.id.clone(),
                                format!("FS err: {}", e),
                            )))
                            .await;
                        return;
                    }
                };

                let mut stream = res.bytes_stream();
                let mut downloaded = current_bytes;
                let mut last_downloaded = current_bytes;
                let mut last_update = std::time::Instant::now();
                let mut stream_interrupted = false;

                // Reset retry attempts once connected and streaming successfully
                attempt = 0;
                let _ = tx
                    .send(Event::Action(Action::SetDownloadDownloading(
                        file.id.clone(),
                    )))
                    .await;
                let _ = tx
                    .send(Event::Action(Action::UpdateDownloadProgress(
                        file.id.clone(),
                        current_bytes,
                        current_total,
                        0.0,
                    )))
                    .await;

                let mut is_first_chunk = true;
                while let Some(chunk_res) = futures_util::StreamExt::next(&mut stream).await {
                    match chunk_res {
                        Ok(chunk) => {
                            if let Err(e) = file_out.write_all(&chunk).await {
                                let _ = tx
                                    .send(Event::Action(Action::DownloadFailed(
                                        file.id.clone(),
                                        format!("Write err: {}", e),
                                    )))
                                    .await;
                                return;
                            }
                            downloaded += chunk.len() as u64;

                            let now = std::time::Instant::now();
                            let elapsed = now.duration_since(last_update).as_secs_f64();
                            if is_first_chunk || elapsed >= 0.1 {
                                is_first_chunk = false;
                                let speed = if elapsed > 0.0 {
                                    (downloaded - last_downloaded) as f64 / elapsed
                                } else {
                                    0.0
                                };
                                let _ = tx
                                    .send(Event::Action(Action::UpdateDownloadProgress(
                                        file.id.clone(),
                                        downloaded,
                                        current_total,
                                        speed,
                                    )))
                                    .await;
                                last_update = now;
                                last_downloaded = downloaded;
                            }
                        }
                        Err(_e) => {
                            // Connection lost / Wi-Fi disconnected while streaming chunks
                            let _ = file_out.flush().await;
                            stream_interrupted = true;
                            break;
                        }
                    }
                }

                if stream_interrupted {
                    let delay = if attempt < backoff_delays.len() {
                        backoff_delays[attempt]
                    } else {
                        10
                    };
                    attempt += 1;
                    let _ = tx
                        .send(Event::Action(Action::SetDownloadReconnecting(
                            file.id.clone(),
                        )))
                        .await;
                    let _ = tx
                        .send(Event::Action(Action::Message(format!(
                            "Connection lost, reconnecting (attempt {})...",
                            attempt
                        ))))
                        .await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    continue;
                }

                if downloaded > last_downloaded {
                    let now = std::time::Instant::now();
                    let elapsed = now.duration_since(last_update).as_secs_f64();
                    let speed = if elapsed > 0.0 {
                        (downloaded - last_downloaded) as f64 / elapsed
                    } else {
                        0.0
                    };
                    let _ = tx
                        .send(Event::Action(Action::UpdateDownloadProgress(
                            file.id.clone(),
                            downloaded,
                            current_total,
                            speed,
                        )))
                        .await;
                }

                let _ = file_out.flush().await;
                let _ = tx
                    .send(Event::Action(Action::CompleteDownload(file.id)))
                    .await;
                return;
            }
            Err(e) => {
                if attempt < max_retries {
                    let delay = if attempt < backoff_delays.len() {
                        backoff_delays[attempt]
                    } else {
                        10
                    };
                    attempt += 1;
                    let _ = tx
                        .send(Event::Action(Action::SetDownloadReconnecting(
                            file.id.clone(),
                        )))
                        .await;
                    let _ = tx
                        .send(Event::Action(Action::Message(format!(
                            "Connection lost, reconnecting (attempt {})...",
                            attempt
                        ))))
                        .await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    continue;
                }

                let _ = tx
                    .send(Event::Action(Action::DownloadFailed(
                        file.id.clone(),
                        format!("Max retries reached: {}", e),
                    )))
                    .await;
                return;
            }
        }
    }
}
