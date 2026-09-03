use crate::drive::models::DriveFile;
use crate::tui::state::{Action, Event};
use reqwest::Client;
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
    let mut req = client.get(&url).bearer_auth(&access_token);

    if start_bytes > 0 {
        req = req.header("Range", format!("bytes={}-", start_bytes));
    }

    match req.send().await {
        Ok(res) if res.status().is_success() || res.status().as_u16() == 206 => {
            let total_size = res
                .content_length()
                .unwrap_or(0)
                .saturating_add(start_bytes);
            let mut stream = res.bytes_stream();

            use tokio::io::AsyncWriteExt;
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let dest_path = format!("{}/Downloads/{}", home, file.name);
            let mut file_out = match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&dest_path)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx
                        .send(Event::Action(Action::Error(format!("FS err: {}", e))))
                        .await;
                    return;
                }
            };

            let mut downloaded = start_bytes;
            let mut last_downloaded = start_bytes;
            let mut last_update = std::time::Instant::now();

            while let Some(chunk_res) = futures_util::StreamExt::next(&mut stream).await {
                match chunk_res {
                    Ok(chunk) => {
                        if let Err(e) = file_out.write_all(&chunk).await {
                            let _ = tx
                                .send(Event::Action(Action::Error(format!("Write err: {}", e))))
                                .await;
                            return;
                        }
                        downloaded += chunk.len() as u64;

                        let now = std::time::Instant::now();
                        let elapsed = now.duration_since(last_update).as_secs_f64();
                        if elapsed >= 0.1 {
                            let speed = if elapsed > 0.0 {
                                (downloaded - last_downloaded) as f64 / elapsed
                            } else {
                                0.0
                            };
                            let _ = tx
                                .send(Event::Action(Action::UpdateDownloadProgress(
                                    file.id.clone(),
                                    downloaded,
                                    total_size,
                                    speed,
                                )))
                                .await;
                            last_update = now;
                            last_downloaded = downloaded;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Event::Action(Action::Error(format!("Stream err: {}", e))))
                            .await;
                        return;
                    }
                }
            }
            let _ = tx
                .send(Event::Action(Action::CompleteDownload(file.id)))
                .await;
        }
        Ok(res) => {
            let _ = tx
                .send(Event::Action(Action::Error(format!(
                    "DL failed: {}",
                    res.status()
                ))))
                .await;
        }
        Err(e) => {
            let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
        }
    }
}
