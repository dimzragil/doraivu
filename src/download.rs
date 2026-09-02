use crate::app::{Action, App, DownloadStatus, DriveFile, Event};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use reqwest::Client;
use tokio::sync::mpsc;

pub async fn download_file_ranged(
    client: Client,
    access_token: String,
    file: DriveFile,
    start_bytes: u64,
    tx: mpsc::Sender<Event>,
) {
    let url = format!(
        "https://www.googleapis.com/drive/v3/files/{}?alt=media",
        file.id
    );
    let mut req = client.get(&url).bearer_auth(&access_token);

    if start_bytes > 0 {
        req = req.header("Range", format!("bytes={}-", start_bytes));
    }

    match req.send().await {
        Ok(res) if res.status().is_success() || res.status().as_u16() == 206 => {
            let total_size = res
                .content_length()
                .unwrap_or(0)
                .saturating_add(start_bytes);
            let mut stream = res.bytes_stream();

            use tokio::io::AsyncWriteExt;
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            let dest_path = format!("{}/Downloads/{}", home, file.name);
            let mut file_out = match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&dest_path)
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx
                        .send(Event::Action(Action::Error(format!("FS err: {}", e))))
                        .await;
                    return;
                }
            };

            let mut downloaded = start_bytes;
            let mut last_downloaded = start_bytes;
            let mut last_update = std::time::Instant::now();

            while let Some(chunk_res) = futures_util::StreamExt::next(&mut stream).await {
                match chunk_res {
                    Ok(chunk) => {
                        if let Err(e) = file_out.write_all(&chunk).await {
                            let _ = tx
                                .send(Event::Action(Action::Error(format!("Write err: {}", e))))
                                .await;
                            return;
                        }
                        downloaded += chunk.len() as u64;

                        let now = std::time::Instant::now();
                        let elapsed = now.duration_since(last_update).as_secs_f64();
                        if elapsed >= 0.1 {
                            let speed = if elapsed > 0.0 {
                                (downloaded - last_downloaded) as f64 / elapsed
                            } else {
                                0.0
                            };
                            let _ = tx
                                .send(Event::Action(Action::UpdateDownloadProgress(
                                    file.id.clone(),
                                    downloaded,
                                    total_size,
                                    speed,
                                )))
                                .await;
                            last_update = now;
                            last_downloaded = downloaded;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Event::Action(Action::Error(format!("Stream err: {}", e))))
                            .await;
                        return;
                    }
                }
            }
            let _ = tx
                .send(Event::Action(Action::CompleteDownload(file.id)))
                .await;
        }
        Ok(res) => {
            let _ = tx
                .send(Event::Action(Action::Error(format!(
                    "DL failed: {}",
                    res.status()
                ))))
                .await;
        }
        Err(e) => {
            let _ = tx.send(Event::Action(Action::Error(e.to_string()))).await;
        }
    }
}

pub fn render_tracker_block(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items: Vec<ListItem> = app
        .dl_manager
        .queue
        .iter()
        .map(|task| {
            let prefix = match task.status {
                DownloadStatus::Pending => "⏳ ",
                DownloadStatus::Downloading => "↓ ",
                DownloadStatus::Paused => "⏸ ",
            };

            let pct = if task.total_bytes > 0 {
                (task.downloaded_bytes as f64 / task.total_bytes as f64) * 100.0
            } else {
                0.0
            };

            let color = match task.status {
                DownloadStatus::Downloading => Color::Green,
                DownloadStatus::Paused => Color::Yellow,
                DownloadStatus::Pending => Color::Gray,
            };

            let status_text = match task.status {
                DownloadStatus::Downloading => {
                    format!("[Downloading] {} {:.1}%", task.file.name, pct)
                }
                DownloadStatus::Paused => format!("[Paused] {} {:.1}%", task.file.name, pct),
                DownloadStatus::Pending => format!("[Pending] {}", task.file.name),
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(color)),
                Span::styled(status_text, Style::default().fg(color)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Download Queue ")
                .title_bottom(
                    ratatui::text::Line::from(
                        " [Esc] Close   [x] Cancel   [p] Pause   [r] Resume ",
                    )
                    .alignment(ratatui::layout::Alignment::Center),
                )
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            ratatui::style::Style::default()
                .bg(ratatui::style::Color::DarkGray)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[0], &mut app.dl_manager.state);

    // BarChart with Braille using Chart widget
    let points: Vec<(f64, f64)> = app
        .dl_manager
        .speed_history
        .iter()
        .enumerate()
        .map(|(i, &speed)| (i as f64, speed as f64 / 1_048_576.0))
        .collect();

    let max_y = points.iter().map(|&(_, y)| y).fold(0.0_f64, f64::max);
    let bound_y = if max_y < 1.0 { 1.0 } else { max_y * 1.2 };

    let datasets = vec![ratatui::widgets::Dataset::default()
        .name("MB/s")
        .marker(ratatui::symbols::Marker::Braille)
        .graph_type(ratatui::widgets::GraphType::Bar)
        .style(Style::default().fg(Color::Green))
        .data(&points)];

    let chart = ratatui::widgets::Chart::new(datasets)
        .block(
            Block::default()
                .title(format!(" Bandwidth (Max: {:.1} MB/s) ", max_y))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .x_axis(
            ratatui::widgets::Axis::default()
                .bounds([0.0, 100.0])
                .style(Style::default().fg(Color::Reset)),
        )
        .y_axis(
            ratatui::widgets::Axis::default()
                .bounds([0.0, bound_y])
                .style(Style::default().fg(Color::Reset)),
        );

    f.render_widget(chart, chunks[1]);
}
