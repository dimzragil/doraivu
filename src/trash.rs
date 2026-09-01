use crate::app::{Action, App, DriveFile, Event};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use reqwest::Client;
use tokio::sync::mpsc;

pub async fn fetch_trash(client: Client, access_token: String, tx: mpsc::Sender<Event>) {
    let url =
        "https://www.googleapis.com/drive/v3/files?q=trashed=true&pageSize=1000&fields=files(id,name,mimeType)";
    match client.get(url).bearer_auth(&access_token).send().await {
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
    match client
        .patch(&url)
        .bearer_auth(&access_token)
        .json(&body)
        .send()
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
    match client.delete(&url).bearer_auth(&access_token).send().await {
        Ok(res) if res.status().is_success() || res.status().as_u16() == 204 => {
            let _ = tx
                .send(Event::Action(Action::Message(
                    "File deleted permanently".into(),
                )))
                .await;
            fetch_trash(client, access_token, tx).await;
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
        if let Ok(res) = client.delete(&url).bearer_auth(&access_token).send().await {
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
    fetch_trash(client, access_token, tx).await;
}

pub fn render_trash_block(f: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .trashed_files
        .iter()
        .map(|file| {
            let icon = if file.mime_type == "application/vnd.google-apps.folder" {
                "📁"
            } else {
                "📄"
            };
            ListItem::new(Line::from(format!("{} {}", icon, file.name)))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Trash ")
                .title_bottom(
                    ratatui::text::Line::from(
                        " [Esc] Close   [r] Restore   [x] Delete   [X] Delete All ",
                    )
                    .alignment(ratatui::layout::Alignment::Center),
                )
                .border_style(Style::default().fg(Color::Red)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, area, &mut app.trash_state);
}
