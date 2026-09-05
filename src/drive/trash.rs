use crate::drive::api::send_with_retry;
use crate::drive::models::DriveFile;
use crate::tui::state::{Action, Event};
use reqwest::Client;
use tokio::sync::mpsc;

pub async fn fetch_trash(client: Client, access_token: String, tx: mpsc::Sender<Event>) {
    let mut all_files = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let mut url = "https://www.googleapis.com/drive/v3/files?q=trashed=true&orderBy=folder,name_natural&pageSize=1000&fields=nextPageToken,files(id,name,mimeType,appProperties)".to_string();
        if let Some(ref pt) = page_token {
            url.push_str(&format!("&pageToken={}", urlencoding::encode(pt)));
        }

        match send_with_retry(|| client.get(&url).bearer_auth(&access_token), 3).await {
            Ok(res) if res.status().is_success() => {
                if let Ok(resp) = res.json::<crate::drive::models::FileListResponse>().await {
                    all_files.extend(resp.files);
                    match resp.next_page_token {
                        Some(token) if !token.trim().is_empty() => {
                            page_token = Some(token);
                        }
                        _ => break,
                    }
                } else {
                    break;
                }
            }
            Ok(res) => {
                let err = res.text().await.unwrap_or_default();
                let _ = tx
                    .send(Event::Action(Action::Error(format!("Trash err: {}", err))))
                    .await;
                return;
            }
            Err(e) => {
                let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
                return;
            }
        }
    }

    crate::drive::api::sort_files(&mut all_files);
    let _ = tx.send(Event::Action(Action::LoadTrash(all_files))).await;
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
