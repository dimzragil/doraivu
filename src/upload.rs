use crate::app::{Action, App, Event, UploadStatus, UploadTask};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
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

pub fn render_tracker_block(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items: Vec<ListItem> = app
        .ul_manager
        .queue
        .iter()
        .map(|task| {
            let prefix = match task.status {
                UploadStatus::Pending => "⏳ ",
                UploadStatus::Uploading => "↑ ",
                UploadStatus::Paused => "⏸ ",
            };

            let pct = if task.total_bytes > 0 {
                (task.uploaded_bytes as f64 / task.total_bytes as f64) * 100.0
            } else {
                0.0
            };

            let color = match task.status {
                UploadStatus::Uploading => Color::Green,
                UploadStatus::Paused => Color::Yellow,
                UploadStatus::Pending => Color::Gray,
            };

            let status_text = match task.status {
                UploadStatus::Uploading => {
                    format!("[Uploading] {} {:.1}%", task.name, pct)
                }
                UploadStatus::Paused => format!("[Paused] {} {:.1}%", task.name, pct),
                UploadStatus::Pending => format!("[Pending] {}", task.name),
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
                .title(" Upload Queue ")
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

    f.render_stateful_widget(list, chunks[0], &mut app.ul_manager.state);

    // BarChart with Braille
    let points: Vec<(f64, f64)> = app
        .ul_manager
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
