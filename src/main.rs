mod cli;
mod drive;
mod tui;

use anyhow::Result;
use crossterm::{
    event::{self, Event as CEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use drive::api::fetch_quota;
use drive::auth::{self, authenticate, AuthInfo};
use drive::models::DriveFile;
use ratatui::{backend::CrosstermBackend, Terminal};
use reqwest::Client;
use std::{
    env, io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::mpsc;
use tui::actions::handle_action;
use tui::events::handle_input;
use tui::layout;
use tui::preview::handle_suspend_and_edit;
use tui::state::{Action, App, Event};

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
    app.load_queues();

    let (tx, mut rx) = mpsc::channel(32);

    // Initialize Virtual Root
    app.files = DriveFile::virtual_root_items();
    app.status = "Virtual Root loaded.".to_string();

    let client_quota = client.clone();
    let token_quota = token.access_token.clone();
    let tx_quota = tx.clone();
    tokio::spawn(async move {
        fetch_quota(client_quota, token_quota, tx_quota).await;
    });

    let is_editing = Arc::new(AtomicBool::new(false));

    // Input thread
    let tx_input = tx.clone();
    let is_editing_input = is_editing.clone();
    tokio::spawn(async move {
        let mut last_tick = std::time::Instant::now();
        loop {
            if is_editing_input.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            if event::poll(Duration::from_millis(250)).unwrap_or(false) {
                if is_editing_input.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                match event::read() {
                    Ok(CEvent::Key(key)) => {
                        if tx_input.send(Event::Input(key)).await.is_err() {
                            break;
                        }
                    }
                    Ok(CEvent::Resize(w, h))
                        if tx_input.send(Event::Resize(w, h)).await.is_err() =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
            if last_tick.elapsed().as_secs() >= 5 {
                if !is_editing_input.load(Ordering::SeqCst)
                    && tx_input.send(Event::Tick).await.is_err()
                {
                    break;
                }
                last_tick = std::time::Instant::now();
            }
        }
    });

    // Main loop
    while !app.should_quit {
        terminal.draw(|f| layout::render(f, &mut app))?;

        if let Some(event) = rx.recv().await {
            match event {
                Event::Tick => {
                    app.save_queues();
                    // Proactive background token refresh before expiry (every 50 minutes / 3000 seconds)
                    if app.token_refreshed_at.elapsed().as_secs() >= 3000
                        && !app.is_refreshing_token
                    {
                        app.is_refreshing_token = true;
                        let client_c = client.clone();
                        let mut token_c = token.clone();
                        let auth_info_c = auth_info.clone();
                        let tx_c = tx.clone();
                        tokio::spawn(async move {
                            if auth::refresh_token_if_needed(&client_c, &auth_info_c, &mut token_c)
                                .await
                                .is_ok()
                            {
                                let _ = tx_c
                                    .send(Event::Action(Action::TokenRefreshed(token_c)))
                                    .await;
                            } else {
                                let _ = tx_c.send(Event::Action(Action::TokenRefreshFailed)).await;
                            }
                        });
                    }
                }
                Event::Resize(_w, _h) => {
                    let _ = terminal.clear();
                }
                Event::Input(key) => {
                    handle_input(&mut app, key, &client, &token, &tx);
                }
                Event::Action(action) => {
                    handle_action(&mut app, action, &client, &mut token, &auth_info, &tx);
                }
                Event::SuspendAndEdit(file) => {
                    if let Err(e) = handle_suspend_and_edit(
                        &mut app,
                        file,
                        &client,
                        &token,
                        &mut terminal,
                        is_editing.clone(),
                    )
                    .await
                    {
                        app.status = format!("Edit error: {}", e);
                    }
                    // Drain any stale events captured during edit transition
                    while let Ok(ev) = rx.try_recv() {
                        if let Event::Action(action) = ev {
                            handle_action(&mut app, action, &client, &mut token, &auth_info, &tx);
                        }
                    }
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
