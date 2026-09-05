use crate::drive::api::send_with_retry;
use crate::drive::models::DriveFile;
use crate::tui::state::{Action, Event};
use reqwest::Client;
use tokio::sync::mpsc;

pub async fn fetch_trash(client: Client, access_token: String, tx: mpsc::Sender<Event>) {
    let url =
        "https://www.googleapis.com/drive/v3/files?q=trashed=true&pageSize=1000&fields=files(id,name,mimeType,appProperties)";
    match send_with_retry(|| client.get(url).bearer_auth(&access_token), 3).await {
        Ok(res) if res.status().is_success() => {
            #[derive(serde::Deserialize)]
            struct FilesResponse {
                files: Vec<DriveFile>,
            }
            if let Ok(data) = res.json::<FilesResponse>().await {
                let _ = tx.send(Event::Action(Action::LoadTrash(data.files))).await;
            }
        }
        Ok(res) => {
            let err = res.text().await.unwrap_or_default();
            let _ = tx
                .send(Event::Action(Action::Error(format!("Trash err: {}", err))))
                .await;
        }
        Err(e) => {
            let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
        }
    }
}

pub async fn restore_file(
    client: Client,
    access_token: String,
    file_id: String,
    tx: mpsc::Sender<Event>,
) {
    let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
    let body = serde_json::json!({"trashed": false});
    match send_with_retry(
        || client.patch(&url).bearer_auth(&access_token).json(&body),
        3,
    )
    .await
    {
        Ok(res) if res.status().is_success() => {
            let _ = tx
                .send(Event::Action(Action::Message("File restored".into())))
                .await;
            // Fetch trash again
            fetch_trash(client, access_token, tx).await;
        }
        _ => {
            let _ = tx
                .send(Event::Action(Action::Error("Failed to restore".into())))
                .await;
        }
    }
}

pub async fn delete_permanently(
    client: Client,
    access_token: String,
    file_id: String,
    tx: mpsc::Sender<Event>,
) {
    let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
    match send_with_retry(|| client.delete(&url).bearer_auth(&access_token), 3).await {
        Ok(res) if res.status().is_success() || res.status().as_u16() == 204 => {
            let _ = tx
                .send(Event::Action(Action::Message(
                    "File deleted permanently".into(),
                )))
                .await;
            fetch_trash(client.clone(), access_token.clone(), tx.clone()).await;
            crate::drive::api::fetch_quota(client, access_token, tx).await;
        }
        _ => {
            let _ = tx
                .send(Event::Action(Action::Error("Failed to delete".into())))
                .await;
        }
    }
}

pub async fn empty_trash(
    client: Client,
    access_token: String,
    files: Vec<DriveFile>,
    tx: mpsc::Sender<Event>,
) {
    let _ = tx
        .send(Event::Action(Action::Message(format!(
            "Deleting {} files...",
            files.len()
        ))))
        .await;
    let mut success_count = 0;

    for file in files {
        let _ = tx
            .send(Event::Action(Action::Message(format!(
                "Deleting {}...",
                file.name
            ))))
            .await;
        let url = format!("https://www.googleapis.com/drive/v3/files/{}", file.id);
        if let Ok(res) = send_with_retry(|| client.delete(&url).bearer_auth(&access_token), 3).await
        {
            if res.status().is_success() || res.status().as_u16() == 204 {
                success_count += 1;
            }
        }
    }

    let _ = tx
        .send(Event::Action(Action::Message(format!(
            "Deleted {} files.",
            success_count
        ))))
        .await;
    fetch_trash(client.clone(), access_token.clone(), tx.clone()).await;
    crate::drive::api::fetch_quota(client, access_token, tx).await;
}
