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
    if let Some(h) = app.preview.active_task.take() {
        h.abort();
    }
    if app.preview.mode == PreviewMode::Hidden {
        return;
    }
    app.preview.state = PreviewState::Loading;
    app.preview.dims = None;
    if let Some(file) = app.selected_file() {
        let c = client.clone();
        let t = token.to_string();
        let fid = file.id.clone();
        let txc = tx.clone();

        if file.mime_type.starts_with("image/") && app.preview.mode == PreviewMode::Default {
            app.preview.active_task = Some(tokio::spawn(async move {
                crate::drive::api::fetch_preview(c, t, fid, txc).await;
            }));
        } else {
            app.preview.active_task = Some(tokio::spawn(async move {
                crate::drive::api::fetch_metadata(c, t, fid, txc).await;
            }));
        }
    }
}

pub fn is_editable_text(mime_type: &str) -> bool {
    if mime_type.starts_with("text/") {
        return true;
    }

    matches!(
        mime_type,
        "application/json"
            | "application/javascript"
            | "application/x-javascript"
            | "application/typescript"
            | "application/x-typescript"
            | "application/x-sh"
            | "application/x-shellscript"
            | "application/x-bash"
            | "application/xml"
            | "application/toml"
            | "application/x-toml"
            | "application/yaml"
            | "application/x-yaml"
            | "application/x-httpd-php"
            | "application/sql"
            | "application/graphql"
            | "application/ld+json"
            | "application/x-subrip"
    )
}

struct TerminalSuspendGuard<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<Stdout>>,
    is_editing: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl<'a> Drop for TerminalSuspendGuard<'a> {
    fn drop(&mut self) {
        let _ = crossterm::terminal::enable_raw_mode();
        let _ = crossterm::execute!(
            self.terminal.backend_mut(),
            crossterm::terminal::EnterAlternateScreen
        );
        let _ = self.terminal.hide_cursor();
        let _ = self.terminal.clear();
        // Drain any stale events captured during transition
        while crossterm::event::poll(std::time::Duration::from_millis(0)).unwrap_or(false) {
            let _ = crossterm::event::read();
        }
        self.is_editing
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

pub async fn handle_suspend_and_edit(
    app: &mut App,
    file: DriveFile,
    client: &Client,
    token: &crate::drive::auth::Token,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    is_editing: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<()> {
    if !is_editable_text(&file.mime_type) {
        app.status = "Cannot edit binary/media files.".into();
        return Ok(());
    }

    // 1. Signal background input loop to pause and stop reading stdin
    is_editing.store(true, std::sync::atomic::Ordering::SeqCst);
    // Give in-flight event::poll time to finish
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // 2. Drain any keys pressed before/during suspension transition
    while crossterm::event::poll(std::time::Duration::from_millis(0)).unwrap_or(false) {
        let _ = crossterm::event::read();
    }

    // 3. Suspend TUI
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // 4. Create guard so terminal & raw mode are GUARANTEED to be restored upon exit
    let _guard = TerminalSuspendGuard {
        terminal,
        is_editing,
    };

    let safe_name = file.name.replace('/', "_");
    let tmp_path = std::env::temp_dir().join(format!("doraivu_edit_{}_{}", file.id, safe_name));
    println!("Downloading {} for editing...", file.name);

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
            tokio::fs::write(&tmp_path, &bytes).await?;

            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
            println!("Opening {} with {}...", file.name, editor);

            let status = tokio::process::Command::new(&editor)
                .arg(&tmp_path)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
                .await;

            match status {
                Ok(exit_status) if exit_status.success() => {
                    let new_content = tokio::fs::read(&tmp_path).await?;
                    if new_content != bytes.as_ref() {
                        println!("File changed, uploading...");

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
                }
                Ok(_) => {
                    app.status = "Editor exited with non-zero status.".into();
                }
                Err(e) => {
                    app.status = format!("Failed to launch editor '{}': {}", editor, e);
                }
            }

            let _ = tokio::fs::remove_file(&tmp_path).await;
        }
        _ => {
            app.status = "Failed to download file for editing.".into();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editable_text_mime_types() {
        assert!(is_editable_text("text/plain"));
        assert!(is_editable_text("text/markdown"));
        assert!(is_editable_text("text/csv"));
        assert!(is_editable_text("text/html"));
        assert!(is_editable_text("text/x-rust"));
        assert!(is_editable_text("application/json"));
        assert!(is_editable_text("application/javascript"));
        assert!(is_editable_text("application/x-sh"));
        assert!(is_editable_text("application/xml"));
        assert!(is_editable_text("application/toml"));
        assert!(is_editable_text("application/yaml"));
    }

    #[test]
    fn test_non_editable_binary_media_types() {
        assert!(!is_editable_text("video/mp4"));
        assert!(!is_editable_text("video/mkv"));
        assert!(!is_editable_text("image/png"));
        assert!(!is_editable_text("image/jpeg"));
        assert!(!is_editable_text("audio/mpeg"));
        assert!(!is_editable_text("application/zip"));
        assert!(!is_editable_text("application/pdf"));
        assert!(!is_editable_text("application/octet-stream"));
        assert!(!is_editable_text("application/vnd.google-apps.folder"));
    }

    #[tokio::test]
    async fn test_trigger_preview_folder_sets_loading_state() {
        let mut app = App::new();
        app.preview.mode = PreviewMode::Default;
        app.files = vec![DriveFile {
            id: "folder_123".to_string(),
            name: "Documents".to_string(),
            mime_type: "application/vnd.google-apps.folder".to_string(),
            app_properties: None,
        }];
        app.state.select(Some(0));

        let client = Client::new();
        let (tx, _rx) = mpsc::channel(1);

        trigger_preview(&mut app, &client, "fake_token", &tx);

        assert!(matches!(app.preview.state, PreviewState::Loading));
    }
}
