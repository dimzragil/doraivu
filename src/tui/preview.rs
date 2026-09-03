use crate::drive::models::DriveFile;
use crate::tui::state::{App, Event, PreviewMode, PreviewState};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::Client;
use std::io::Stdout;
use tokio::sync::mpsc;

pub fn trigger_preview(app: &mut App, client: &Client, token: &str, tx: &mpsc::Sender<Event>) {
    if app.preview_mode == PreviewMode::Hidden {
        return;
    }
    app.preview_state = PreviewState::Loading;
    app.preview_dims = None;
    if let Some(file) = app.selected_file() {
        let c = client.clone();
        let t = token.to_string();
        let fid = file.id.clone();
        let txc = tx.clone();

        if file.mime_type.starts_with("image/") && app.preview_mode == PreviewMode::Default {
            tokio::spawn(async move {
                crate::drive::api::fetch_preview(c, t, fid, txc).await;
            });
        } else if file.mime_type != "application/vnd.google-apps.folder" {
            tokio::spawn(async move {
                crate::drive::api::fetch_metadata(c, t, fid, txc).await;
            });
        } else {
            app.preview_state = PreviewState::None;
        }
    }
}

pub async fn handle_suspend_and_edit(
    app: &mut App,
    file: DriveFile,
    client: &Client,
    token: &crate::drive::auth::Token,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> anyhow::Result<()> {
    // Suspend UI
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    let tmp_path = format!("/tmp/doraivu_edit_{}", file.name);
    println!("Downloading {} for editing...", file.name);

    // Blocking download
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?alt=media",
        file.id
    );
    match client
        .get(&url)
        .bearer_auth(&token.access_token)
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            let bytes = res.bytes().await?;
            std::fs::write(&tmp_path, &bytes)?;

            let before_metadata = std::fs::metadata(&tmp_path)?;

            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
            let mut child = tokio::process::Command::new(editor)
                .arg(&tmp_path)
                .spawn()?;

            child.wait().await?;

            let after_metadata = std::fs::metadata(&tmp_path)?;
            if after_metadata.modified()? > before_metadata.modified()? {
                println!("File changed, uploading...");

                let new_content = std::fs::read(&tmp_path)?;
                let upload_url = format!(
                    "https://www.googleapis.com/upload/drive/v3/files/{}?uploadType=media",
                    file.id
                );
                let patch_res = client
                    .patch(&upload_url)
                    .bearer_auth(&token.access_token)
                    .header("Content-Type", &file.mime_type)
                    .body(new_content)
                    .send()
                    .await?;

                if patch_res.status().is_success() {
                    app.status = format!("Successfully updated {}", file.name);
                } else {
                    app.status = format!("Upload failed: {}", patch_res.status());
                }
            } else {
                app.status = "No changes made.".into();
            }
            let _ = std::fs::remove_file(&tmp_path);
        }
        _ => {
            app.status = "Failed to download file for editing.".into();
        }
    }

    Ok(())
}
