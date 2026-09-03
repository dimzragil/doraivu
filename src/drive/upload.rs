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

    let file_result = tokio::fs::File::open(&local_path).await;
    let file = match file_result {
        Ok(f) => f,
        Err(e) => {
            let _ = tx
                .send(Event::Action(Action::Error(format!("Open err: {}", e))))
                .await;
            return;
        }
    };

    let total_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);

    let (body_tx, body_rx) = tokio::sync::mpsc::channel(1);
    let stream = tokio_stream::wrappers::ReceiverStream::new(body_rx);

    let tx_clone = tx.clone();
    let id_clone = local_path.clone();

    tokio::spawn(async move {
        let mut framed = FramedRead::new(file, BytesCodec::new());
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
                    if elapsed >= 0.1 {
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

    let metadata_str = serde_json::to_string(&metadata).unwrap();

    let part_metadata = reqwest::multipart::Part::text(metadata_str)
        .mime_str("application/json")
        .unwrap();

    let body = reqwest::Body::wrap_stream(stream);
    let part_file = reqwest::multipart::Part::stream_with_length(body, total_size)
        .file_name(task.name.clone())
        .mime_str("application/octet-stream")
        .unwrap();

    let form = reqwest::multipart::Form::new()
        .part("metadata", part_metadata)
        .part("file", part_file);

    let url = "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart";
    match client
        .post(url)
        .bearer_auth(&access_token)
        .multipart(form)
        .send()
        .await
    {
        Ok(res) => {
            if res.status().is_success() {
                let _ = tx
                    .send(Event::Action(Action::CompleteUpload(local_path)))
                    .await;
            } else {
                let _ = tx
                    .send(Event::Action(Action::Error(format!(
                        "Upload failed: {}",
                        res.status()
                    ))))
                    .await;
            }
        }
        Err(e) => {
            let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
        }
    }
}
