mod api;
mod app;
mod auth;
mod ui;

use anyhow::Result;
use api::{download_file, fetch_files, fetch_preview, fetch_quota, upload_file};
use app::{Action, App, Event};
use auth::{authenticate, AuthInfo};
use crossterm::{
    event::{self, Event as CEvent, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::Client;
use std::{env, io, time::Duration};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    let mut arg_client_id = None;
    let mut arg_client_secret = None;
    for arg in env::args().skip(1) {
        if let Some(val) = arg.strip_prefix("GOOGLE_CLIENT_ID=") {
            arg_client_id = Some(val.to_string());
        } else if let Some(val) = arg.strip_prefix("GOOGLE_CLIENT_SECRET=") {
            arg_client_secret = Some(val.to_string());
        }
    }

    let auth_info = if let (Some(client_id), Some(client_secret)) =
        (arg_client_id, arg_client_secret)
    {
        let info = AuthInfo {
            client_id,
            client_secret,
        };
        let _ = auth::save_credentials(&info);
        // Force re-login if credentials changed
        let _ = std::fs::remove_file(auth::get_token_path().unwrap_or_default());
        info
    } else {
        match auth::load_credentials() {
            Ok(Some(info)) => info,
            _ => {
                let client_id = env::var("GOOGLE_CLIENT_ID").unwrap_or_else(|_| "".to_string());
                let client_secret =
                    env::var("GOOGLE_CLIENT_SECRET").unwrap_or_else(|_| "".to_string());
                if client_id.is_empty() || client_secret.is_empty() {
                    anyhow::bail!("No credentials found.\nPlease run: doraivu GOOGLE_CLIENT_ID=\"...\" GOOGLE_CLIENT_SECRET=\"...\"");
                }
                let info = AuthInfo {
                    client_id,
                    client_secret,
                };
                let _ = auth::save_credentials(&info);
                info
            }
        }
    };

    let client = Client::builder()
        .user_agent("doraivu-rust-client/1.0")
        .build()?;

    let mut token = authenticate(&client, &auth_info).await?;

    // Check if token expired, refresh if needed
    // Simple naive check - in production you'd use expiration time
    if let Err(e) = client
        .get("https://www.googleapis.com/drive/v3/about?fields=user")
        .bearer_auth(&token.access_token)
        .send()
        .await?
        .error_for_status()
    {
        if let Some(status) = e.status() {
            if status == reqwest::StatusCode::UNAUTHORIZED {
                auth::refresh_token_if_needed(&client, &auth_info, &mut token).await?;
            }
        }
    }

    // Setup terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    let (tx, mut rx) = mpsc::channel(32);

    // Initial fetch
    let client_clone = client.clone();
    let token_clone = token.access_token.clone();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let q = "'root' in parents and trashed = false".to_string();
        fetch_files(client_clone, token_clone, q, tx_clone).await;
    });

    let client_quota = client.clone();
    let token_quota = token.access_token.clone();
    let tx_quota = tx.clone();
    tokio::spawn(async move {
        fetch_quota(client_quota, token_quota, tx_quota).await;
    });

    // Input thread
    let tx_input = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(250)).unwrap_or(false) {
                if let Ok(CEvent::Key(key)) = event::read() {
                    if tx_input.send(Event::Input(key)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Main loop
    while !app.should_quit {
        terminal.draw(|f| ui::render(f, &mut app))?;

        if let Some(event) = rx.recv().await {
            match event {
                Event::Input(key) => match app.input_mode {
                    app::InputMode::DeleteConfirmModal => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                            if let Some(file) = app.selected_file().cloned() {
                                app.status = format!("Trashing {}...", file.name);
                                let c = client.clone();
                                let t = token.access_token.clone();
                                let txc = tx.clone();
                                if let Some(i) = app.state.selected() {
                                    app.files.remove(i);
                                }
                                tokio::spawn(async move {
                                    api::trash_file(c, t, file.id, txc).await;
                                });
                            }
                            app.input_mode = app::InputMode::Normal;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            app.input_mode = app::InputMode::Normal;
                        }
                        _ => {}
                    },
                    app::InputMode::UploadModal => match key.code {
                        KeyCode::Esc => {
                            app.input_mode = app::InputMode::Normal;
                        }
                        KeyCode::Tab => {
                            app.upload_input_idx = (app.upload_input_idx + 1) % 2;
                        }
                        KeyCode::Enter => {
                            let path_str = if app.path_names.len() == 1 {
                                "/".to_string()
                            } else {
                                format!("/{}", app.path_names[1..].join("/"))
                            };
                            let parent = if app.upload_target_id == path_str {
                                app.current_path.clone()
                            } else {
                                app.upload_target_id.clone()
                            };
                            let path = app.upload_local_path.clone();
                            app.upload_progress = Some((0, 0, 0.0));
                            app.input_mode = app::InputMode::Normal;
                            app.upload_local_path.clear();
                            let c = client.clone();
                            let t = token.access_token.clone();
                            let txc = tx.clone();
                            tokio::spawn(async move {
                                upload_file(c, t, parent, path, txc).await;
                            });
                        }
                        KeyCode::Backspace => {
                            if app.upload_input_idx == 0 {
                                app.upload_target_id.pop();
                            } else {
                                app.upload_local_path.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if app.upload_input_idx == 0 {
                                app.upload_target_id.push(c);
                            } else {
                                app.upload_local_path.push(c);
                            }
                        }
                        _ => {}
                    },
                    app::InputMode::Normal => {
                        if app.search_mode {
                            match key.code {
                                KeyCode::Esc => {
                                    app.search_mode = false;
                                }
                                KeyCode::Enter => {
                                    app.search_mode = false;
                                }
                                KeyCode::Backspace => {
                                    app.search_query.pop();
                                    let c = client.clone();
                                    let t = token.access_token.clone();
                                    let sq = app.search_query.clone().replace("'", "\\'");
                                    let txc = tx.clone();
                                    tokio::spawn(async move {
                                        let q = if sq.is_empty() {
                                            "'root' in parents and trashed = false".to_string()
                                        } else {
                                            format!("name contains '{}' and trashed = false", sq)
                                        };
                                        fetch_files(c, t, q, txc).await;
                                    });
                                }
                                KeyCode::Char(c) => {
                                    app.search_query.push(c);
                                    let cl = client.clone();
                                    let t = token.access_token.clone();
                                    let sq = app.search_query.clone().replace("'", "\\'");
                                    let txc = tx.clone();
                                    tokio::spawn(async move {
                                        let q =
                                            format!("name contains '{}' and trashed = false", sq);
                                        fetch_files(cl, t, q, txc).await;
                                    });
                                }
                                _ => {}
                            }
                        } else {
                            match key.code {
                                KeyCode::Char('q') => app.should_quit = true,
                                KeyCode::Char('j') | KeyCode::Down => {
                                    app.next();
                                    trigger_preview(&mut app, &client, &token.access_token, &tx);
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    app.previous();
                                    trigger_preview(&mut app, &client, &token.access_token, &tx);
                                }
                                KeyCode::Char('p') => {
                                    app.show_preview = !app.show_preview;
                                    trigger_preview(&mut app, &client, &token.access_token, &tx);
                                }
                                KeyCode::Char('r') => {
                                    app.status = "Refreshing...".into();
                                    app.files.clear();
                                    let c = client.clone();
                                    let t = token.access_token.clone();
                                    let txc = tx.clone();
                                    let parent_id = app.current_path.clone();
                                    tokio::spawn(async move {
                                        let q = format!(
                                            "'{}' in parents and trashed = false",
                                            parent_id
                                        );
                                        fetch_files(c.clone(), t.clone(), q, txc.clone()).await;
                                        fetch_quota(c, t, txc).await;
                                    });
                                }
                                KeyCode::Char('/') => {
                                    app.search_mode = true;
                                    app.search_query.clear();
                                }
                                KeyCode::Char('v') | KeyCode::Char('m') => {
                                    if let Some(file) = app.selected_file().cloned() {
                                        if !file.mime_type.ends_with("folder") {
                                            app.status =
                                                format!("Starting mpv for {}...", file.name);
                                            let url = format!("https://www.googleapis.com/drive/v3/files/{}?alt=media", file.id);
                                            let token_str = token.access_token.clone();
                                            tokio::spawn(async move {
                                                let _ = tokio::process::Command::new("mpv")
                                                    .arg(&url)
                                                    .arg(format!(
                                                    "--http-header-fields=Authorization: Bearer {}",
                                                    token_str
                                                ))
                                                    .stdout(std::process::Stdio::null())
                                                    .stderr(std::process::Stdio::null())
                                                    .spawn();
                                            });
                                        }
                                    }
                                }
                                KeyCode::Enter | KeyCode::Char('l') => {
                                    if let Some(file) = app.selected_file().cloned() {
                                        if file.mime_type == "application/vnd.google-apps.folder" {
                                            app.history.push(app.current_path.clone());
                                            app.path_names.push(file.name.clone());
                                            app.current_path = file.id.clone();
                                            app.status =
                                                format!("Loading folder: {}...", file.name);
                                            app.files.clear(); // Optional: clear while loading

                                            let c = client.clone();
                                            let t = token.access_token.clone();
                                            let fid = file.id.clone();
                                            let txc = tx.clone();

                                            tokio::spawn(async move {
                                                let q = format!(
                                                    "'{}' in parents and trashed = false",
                                                    fid
                                                );
                                                fetch_files(c, t, q, txc).await;
                                            });
                                        } else {
                                            app.status = format!("Cannot open file: {}", file.name);
                                        }
                                    }
                                }
                                KeyCode::Char('h') | KeyCode::Backspace => {
                                    if let Some(parent_id) = app.history.pop() {
                                        app.path_names.pop();
                                        app.current_path = parent_id.clone();
                                        app.status = "Going back...".into();
                                        app.files.clear();

                                        let c = client.clone();
                                        let t = token.access_token.clone();
                                        let txc = tx.clone();

                                        tokio::spawn(async move {
                                            let q = format!(
                                                "'{}' in parents and trashed = false",
                                                parent_id
                                            );
                                            fetch_files(c, t, q, txc).await;
                                        });
                                    } else {
                                        app.status = "Already at root.".into();
                                    }
                                }
                                KeyCode::Char('x') | KeyCode::Delete => {
                                    if app.selected_file().is_some() {
                                        app.input_mode = app::InputMode::DeleteConfirmModal;
                                    }
                                }
                                KeyCode::Char('d') => {
                                    if let Some(file) = app.selected_file().cloned() {
                                        if file.mime_type != "application/vnd.google-apps.folder" {
                                            app.download_progress = Some((0, 0, 0.0));
                                            let c = client.clone();
                                            let t = token.access_token.clone();
                                            let txc = tx.clone();
                                            tokio::spawn(async move {
                                                download_file(c, t, file, txc).await;
                                            });
                                        } else {
                                            app.status =
                                                "Cannot download folders directly yet.".into();
                                        }
                                    }
                                }
                                KeyCode::Char('u') => {
                                    app.input_mode = app::InputMode::UploadModal;
                                    if app.path_names.len() == 1 {
                                        app.upload_target_id = "/".to_string();
                                    } else {
                                        app.upload_target_id =
                                            format!("/{}", app.path_names[1..].join("/"));
                                    }
                                    app.upload_input_idx = 1; // focus local path
                                    app.upload_local_path.clear();
                                }
                                KeyCode::Char('e') => {
                                    if let Some(file) = app.selected_file().cloned() {
                                        if file.mime_type != "application/vnd.google-apps.folder" {
                                            let _ = tx.try_send(Event::SuspendAndEdit(file));
                                        } else {
                                            app.status = "Cannot edit folders.".into();
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                },
                Event::Action(action) => match action {
                    Action::LoadFiles(files) => {
                        app.files = files;
                        app.status = format!("Loaded {} items.", app.files.len());
                        app.state
                            .select(if app.files.is_empty() { None } else { Some(0) });
                    }
                    Action::Error(err) => {
                        app.status = format!("Error: {}", err);
                        app.download_progress = None;
                        app.upload_progress = None;
                    }
                    Action::DownloadProgress(dl, total, speed) => {
                        app.download_progress = Some((dl, total, speed));
                    }
                    Action::DownloadComplete(msg) => {
                        app.download_progress = None;
                        app.status = msg;
                    }
                    Action::Message(msg) => {
                        app.status = msg;
                    }
                    Action::LoadQuota(used, limit) => {
                        app.storage_quota = Some((used, limit));
                    }
                    Action::UploadProgress(up, total, speed) => {
                        app.upload_progress = Some((up, total, speed));
                    }
                    Action::UploadComplete(msg) => {
                        app.upload_progress = None;
                        app.status = msg;
                    }
                    Action::ImagePreview(bytes) => {
                        if app.show_preview {
                            if let Ok(img) = image::load_from_memory(&bytes) {
                                app.preview_dims = Some((img.width(), img.height()));
                                app.preview_image = Some(app.picker.new_resize_protocol(img));
                            }
                        }
                    }
                },
                Event::SuspendAndEdit(file) => {
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

                            let editor =
                                std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
                            let mut child = tokio::process::Command::new(editor)
                                .arg(&tmp_path)
                                .spawn()?;

                            child.wait().await?;

                            let after_metadata = std::fs::metadata(&tmp_path)?;
                            if after_metadata.modified()? > before_metadata.modified()? {
                                println!("File changed, uploading...");

                                let new_content = std::fs::read(&tmp_path)?;
                                let upload_url = format!("https://www.googleapis.com/upload/drive/v3/files/{}?uploadType=media", file.id);
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

                    // Restore UI
                    enable_raw_mode()?;
                    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                    terminal.clear()?;
                }
            }
        }
    }

    // Teardown
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    std::process::exit(0);
}

fn trigger_preview(app: &mut App, client: &Client, token: &str, tx: &mpsc::Sender<Event>) {
    if !app.show_preview {
        return;
    }
    app.preview_image = None; // clear old preview
    app.preview_dims = None;
    if let Some(file) = app.selected_file() {
        if file.mime_type.starts_with("image/") {
            let c = client.clone();
            let t = token.to_string();
            let fid = file.id.clone();
            let txc = tx.clone();
            tokio::spawn(async move {
                fetch_preview(c, t, fid, txc).await;
            });
        }
    }
}
