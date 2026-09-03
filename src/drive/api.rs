use crate::drive::models::{DriveFile, UploadStatus, UploadTask};
use crate::tui::state::{Action, Event};
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
        "https://www.googleapis.com/drive/v3/files?q={}&fields=files(id,name,mimeType,appProperties)",
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

/// Fetches storage quota information
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

#[derive(Debug, PartialEq, Eq)]
pub enum DriveBaseFolder {
    Root,
    SharedWithMe(String),
    DirectId(String),
}

/// Parses a user-supplied target path string into a base Drive location and remaining subfolders.
/// Handles virtual root, "/My Drive", "/Shared with me", direct IDs, and relative/absolute paths.
pub fn parse_drive_path(path: &str) -> (DriveBaseFolder, Vec<String>) {
    let trimmed = path.trim();

    if trimmed.is_empty()
        || trimmed == "/"
        || trimmed == "root"
        || trimmed == "virtual_root"
        || trimmed == "/My Drive"
        || trimmed == "My Drive"
    {
        return (DriveBaseFolder::Root, Vec::new());
    }

    // If it doesn't contain '/' and doesn't start with '/', it is a direct Google Drive folder ID
    if !trimmed.starts_with('/') && !trimmed.contains('/') {
        return (DriveBaseFolder::DirectId(trimmed.to_string()), Vec::new());
    }

    let parts: Vec<&str> = trimmed
        .trim_matches('/')
        .split('/')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        return (DriveBaseFolder::Root, Vec::new());
    }

    if parts[0].eq_ignore_ascii_case("My Drive") {
        (
            DriveBaseFolder::Root,
            parts[1..].iter().map(|s| s.to_string()).collect(),
        )
    } else if parts[0].eq_ignore_ascii_case("Shared with me")
        || parts[0].eq_ignore_ascii_case("shared_with_me")
    {
        if parts.len() < 2 {
            (DriveBaseFolder::SharedWithMe(String::new()), Vec::new())
        } else {
            (
                DriveBaseFolder::SharedWithMe(parts[1].to_string()),
                parts[2..].iter().map(|s| s.to_string()).collect(),
            )
        }
    } else {
        (
            DriveBaseFolder::Root,
            parts.iter().map(|s| s.to_string()).collect(),
        )
    }
}

/// Resolves a path like "/My Drive/folder/subfolder" into a Google Drive folder ID,
/// creating folders along the way if they don't exist.
async fn resolve_or_create_path(
    client: &Client,
    access_token: &str,
    path: &str,
    tx: &mpsc::Sender<Event>,
) -> anyhow::Result<String> {
    let (base, remaining) = parse_drive_path(path);

    let mut current_id = match base {
        DriveBaseFolder::DirectId(id) => return Ok(id),
        DriveBaseFolder::Root => "root".to_string(),
        DriveBaseFolder::SharedWithMe(shared_folder) => {
            if shared_folder.is_empty() {
                anyhow::bail!(
                    "Cannot upload directly to 'Shared with me' root. Please specify a shared folder name (e.g. /Shared with me/FolderName)"
                );
            }
            let _ = tx
                .send(Event::Action(Action::UploadComplete(format!(
                    "Resolving shared folder {}...",
                    shared_folder
                ))))
                .await;

            let query = format!(
                "mimeType = 'application/vnd.google-apps.folder' and name = '{}' and sharedWithMe = true and trashed = false",
                shared_folder.replace('\'', "\\'")
            );
            let url = format!(
                "https://www.googleapis.com/drive/v3/files?q={}&fields=files(id)",
                urlencoding::encode(&query)
            );

            let res = client.get(&url).bearer_auth(access_token).send().await?;
            if !res.status().is_success() {
                anyhow::bail!("Failed to query shared folder '{}'", shared_folder);
            }

            let data: serde_json::Value = res.json().await?;
            let found_id = data["files"]
                .as_array()
                .and_then(|files| files.first())
                .and_then(|f| f["id"].as_str())
                .map(|id| id.to_string());

            match found_id {
                Some(id) => id,
                None => {
                    anyhow::bail!(
                        "Shared folder '{}' not found in 'Shared with me'",
                        shared_folder
                    );
                }
            }
        }
    };

    for part in remaining {
        let _ = tx
            .send(Event::Action(Action::UploadComplete(format!(
                "Resolving {}...",
                part
            ))))
            .await;

        let query = format!(
            "mimeType = 'application/vnd.google-apps.folder' and name = '{}' and '{}' in parents and trashed = false",
            part.replace('\'', "\\'"),
            current_id
        );
        let url = format!(
            "https://www.googleapis.com/drive/v3/files?q={}&fields=files(id)",
            urlencoding::encode(&query)
        );

        let res = client.get(&url).bearer_auth(access_token).send().await?;
        if !res.status().is_success() {
            anyhow::bail!("API error resolving folder '{}'", part);
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
            anyhow::bail!("Failed to create folder '{}'", part);
        }

        let data: serde_json::Value = res.json().await?;
        if let Some(id) = data["id"].as_str() {
            current_id = id.to_string();
        } else {
            anyhow::bail!("No ID returned when creating folder '{}'", part);
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
                    let task = UploadTask {
                        local_path: path.to_string_lossy().to_string(),
                        name,
                        target_parent_id: current_parent_id.clone(),
                        total_bytes,
                        uploaded_bytes: 0,
                        status: UploadStatus::Pending,
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
        let task = UploadTask {
            local_path,
            name: file_name,
            target_parent_id: parent_id,
            total_bytes,
            uploaded_bytes: 0,
            status: UploadStatus::Pending,
        };
        let _ = tx
            .send(Event::Action(Action::QueueUploads(vec![task])))
            .await;
    }
}

pub async fn fetch_metadata(
    client: Client,
    access_token: String,
    file_id: String,
    tx: mpsc::Sender<Event>,
) {
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?fields=id,name,mimeType,size,createdTime,modifiedTime,appProperties",
        file_id
    );
    match client.get(&url).bearer_auth(&access_token).send().await {
        Ok(res) if res.status().is_success() => {
            #[derive(serde::Deserialize)]
            struct MetadataResponse {
                name: String,
                size: Option<String>,
                #[serde(rename = "createdTime")]
                created_time: Option<String>,
                #[serde(rename = "modifiedTime")]
                modified_time: Option<String>,
            }
            if let Ok(data) = res.json::<MetadataResponse>().await {
                let size = data.size.and_then(|s| s.parse::<u64>().ok());
                let created = data.created_time.unwrap_or_else(|| "Unknown".to_string());
                let modified = data.modified_time.unwrap_or_else(|| "Unknown".to_string());
                let _ = tx
                    .send(Event::Action(Action::PreviewMetadataLoaded(
                        data.name, size, created, modified,
                    )))
                    .await;
            }
        }
        _ => {}
    }
}

pub async fn update_resume_time(
    client: Client,
    access_token: String,
    file_id: String,
    resume_time: String,
) -> anyhow::Result<()> {
    let url = format!("https://www.googleapis.com/drive/v3/files/{}", file_id);
    let payload = serde_json::json!({
        "appProperties": {
            "mpv_resume_time": resume_time
        }
    });

    let res = client
        .patch(&url)
        .bearer_auth(&access_token)
        .json(&payload)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("Failed to update resume time: {} - {}", status, body);
    }

    Ok(())
}

pub fn format_time(rfc3339: &str) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339) {
        dt.with_timezone(&chrono::Local)
            .format("%d %b %Y, %I:%M %p")
            .to_string()
    } else {
        rfc3339.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_drive_path_root_and_virtual_root() {
        assert_eq!(parse_drive_path("/"), (DriveBaseFolder::Root, vec![]));
        assert_eq!(parse_drive_path(""), (DriveBaseFolder::Root, vec![]));
        assert_eq!(parse_drive_path("root"), (DriveBaseFolder::Root, vec![]));
        assert_eq!(
            parse_drive_path("virtual_root"),
            (DriveBaseFolder::Root, vec![])
        );
        assert_eq!(
            parse_drive_path("/My Drive"),
            (DriveBaseFolder::Root, vec![])
        );
        assert_eq!(
            parse_drive_path("My Drive"),
            (DriveBaseFolder::Root, vec![])
        );
    }

    #[test]
    fn test_parse_drive_path_subfolders_under_my_drive() {
        assert_eq!(
            parse_drive_path("/My Drive/Photos"),
            (DriveBaseFolder::Root, vec!["Photos".to_string()])
        );
        assert_eq!(
            parse_drive_path("/My Drive/Photos/2026"),
            (
                DriveBaseFolder::Root,
                vec!["Photos".to_string(), "2026".to_string()]
            )
        );
        assert_eq!(
            parse_drive_path("/Photos/2026"),
            (
                DriveBaseFolder::Root,
                vec!["Photos".to_string(), "2026".to_string()]
            )
        );
    }

    #[test]
    fn test_parse_drive_path_shared_with_me() {
        assert_eq!(
            parse_drive_path("/Shared with me/TeamProjects"),
            (
                DriveBaseFolder::SharedWithMe("TeamProjects".to_string()),
                vec![]
            )
        );
        assert_eq!(
            parse_drive_path("/Shared with me/TeamProjects/Docs"),
            (
                DriveBaseFolder::SharedWithMe("TeamProjects".to_string()),
                vec!["Docs".to_string()]
            )
        );
    }

    #[test]
    fn test_parse_drive_path_direct_id() {
        assert_eq!(
            parse_drive_path("1aBcDeFg_12345"),
            (
                DriveBaseFolder::DirectId("1aBcDeFg_12345".to_string()),
                vec![]
            )
        );
    }
}
