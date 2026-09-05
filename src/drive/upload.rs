use crate::drive::models::UploadTask;
use crate::tui::state::{Action, Event};
use reqwest::Client;
use tokio::sync::mpsc;
use tokio_util::codec::{BytesCodec, FramedRead};

pub async fn upload_file_task(
    client: Client,
    access_token: String,
    task: UploadTask,
    tx: mpsc::Sender<Event>,
) {
    let local_path = task.local_path.clone();
    let backoff_delays = [3, 5, 10, 10, 10, 10, 10, 10];
    let max_retries = backoff_delays.len();
    let mut attempt = 0;

    loop {
        let file = match tokio::fs::File::open(&local_path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = tx
                    .send(Event::Action(Action::Error(format!("Open err: {}", e))))
                    .await;
                return;
            }
        };

        let total_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

        // Buffer up to 32 chunks (approx 2MB) to prevent TCP starvation and cwnd collapse
        let (body_tx, body_rx) = tokio::sync::mpsc::channel(32);
        let stream = tokio_stream::wrappers::ReceiverStream::new(body_rx);

        let tx_clone = tx.clone();
        let id_clone = local_path.clone();

        let reader_handle = tokio::spawn(async move {
            // Read in 64 KB chunks to efficiently feed TCP socket send buffers
            let mut framed = FramedRead::with_capacity(file, BytesCodec::new(), 64 * 1024);
            let mut uploaded = 0;
            let mut last_uploaded = 0u64;
            let mut last_update = std::time::Instant::now();

            while let Some(chunk_res) = futures_util::StreamExt::next(&mut framed).await {
                match chunk_res {
                    Ok(bytes) => {
                        let len = bytes.len() as u64;
                        if body_tx
                            .send(Ok::<_, std::io::Error>(bytes.freeze()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        uploaded += len;

                        let now = std::time::Instant::now();
                        let elapsed = now.duration_since(last_update).as_secs_f64();
                        if elapsed >= 0.2 {
                            let speed = if elapsed > 0.0 {
                                (uploaded - last_uploaded) as f64 / elapsed
                            } else {
                                0.0
                            };
                            let _ = tx_clone
                                .send(Event::Action(Action::UpdateUploadProgress(
                                    id_clone.clone(),
                                    uploaded,
                                    total_size,
                                    speed,
                                )))
                                .await;
                            last_update = now;
                            last_uploaded = uploaded;
                        }
                    }
                    Err(e) => {
                        let _ = tx_clone
                            .send(Event::Action(Action::Error(format!("Read err: {}", e))))
                            .await;
                        break;
                    }
                }
            }
        });

        let metadata = serde_json::json!({
            "name": task.name,
            "parents": [task.target_parent_id]
        });

        let metadata_str = match serde_json::to_string(&metadata) {
            Ok(s) => s,
            Err(e) => {
                reader_handle.abort();
                let _ = tx
                    .send(Event::Action(Action::Error(format!(
                        "Failed to serialize upload metadata: {}",
                        e
                    ))))
                    .await;
                return;
            }
        };

        let part_metadata =
            match reqwest::multipart::Part::text(metadata_str).mime_str("application/json") {
                Ok(p) => p,
                Err(e) => {
                    reader_handle.abort();
                    let _ = tx
                        .send(Event::Action(Action::Error(format!(
                            "Failed to parse json mime type: {}",
                            e
                        ))))
                        .await;
                    return;
                }
            };

        let body = reqwest::Body::wrap_stream(stream);
        let part_file = match reqwest::multipart::Part::stream_with_length(body, total_size)
            .file_name(task.name.clone())
            .mime_str("application/octet-stream")
        {
            Ok(p) => p,
            Err(e) => {
                reader_handle.abort();
                let _ = tx
                    .send(Event::Action(Action::Error(format!(
                        "Failed to parse octet-stream mime type: {}",
                        e
                    ))))
                    .await;
                return;
            }
        };

        let form = reqwest::multipart::Form::new()
            .part("metadata", part_metadata)
            .part("file", part_file);

        let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";
        let res = client
            .post(url)
            .bearer_auth(&access_token)
            .multipart(form)
            .send()
            .await;

        reader_handle.abort();

        match res {
            Ok(res) => {
                if res.status().is_success() {
                    let _ = tx
                        .send(Event::Action(Action::CompleteUpload(local_path)))
                        .await;
                    return;
                }

                let status = res.status();
                if status.as_u16() == 401 {
                    let _ = tx
                        .send(Event::Action(Action::Error("401 Unauthorized".to_string())))
                        .await;
                    return;
                }

                if (status.is_server_error() || status.as_u16() == 429) && attempt < max_retries {
                    let delay = backoff_delays[attempt];
                    attempt += 1;
                    let _ = tx
                        .send(Event::Action(Action::SetUploadReconnecting(
                            local_path.clone(),
                        )))
                        .await;
                    let _ = tx
                        .send(Event::Action(Action::Message(
                            "Connection lost, reconnecting...".into(),
                        )))
                        .await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    continue;
                }

                let body = res.text().await.unwrap_or_default();
                let _ = tx
                    .send(Event::Action(Action::Error(format!(
                        "Upload failed ({}): {}",
                        status, body
                    ))))
                    .await;
                return;
            }
            Err(e) => {
                if attempt < max_retries {
                    let delay = backoff_delays[attempt];
                    attempt += 1;
                    let _ = tx
                        .send(Event::Action(Action::SetUploadReconnecting(
                            local_path.clone(),
                        )))
                        .await;
                    let _ = tx
                        .send(Event::Action(Action::Message(
                            "Connection lost, reconnecting...".into(),
                        )))
                        .await;
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    continue;
                }

                let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
                return;
            }
        }
    }
}
