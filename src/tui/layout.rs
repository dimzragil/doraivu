use crate::drive::models::{DownloadStatus, DriveFile, UploadStatus};
use crate::tui::state::{App, ClipboardAction, InputMode, PreviewMode, PreviewState};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::collections::{HashSet, VecDeque};

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.1}TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

fn centered_rect(percent_x: u16, fixed_height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(r.height.saturating_sub(fixed_height) / 2),
            Constraint::Length(fixed_height),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Renders a bandwidth history chart (shared between download and upload queues)
fn render_bandwidth_chart(
    f: &mut Frame,
    area: Rect,
    speed_history: &VecDeque<u64>,
    theme_color: Color,
) {
    let points: Vec<(f64, f64)> = speed_history
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
                .border_style(Style::default().fg(theme_color)),
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

    f.render_widget(chart, area);
}

/// Renders the trash file list
pub fn render_trash_block(f: &mut Frame, area: Rect, app: &mut App) {
    let theme_color = app.theme_color;
    let items: Vec<ListItem> = app
        .trash
        .files
        .iter()
        .map(|file| {
            let (icon, style) =
                crate::tui::icons::get_file_meta(&file.name, &file.mime_type, theme_color);
            ListItem::new(Line::from(vec![
                Span::styled(icon, style),
                Span::styled(&file.name, style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Trash ")
                .title_bottom(
                    Line::from(" [Esc] Close   [r] Restore   [x] Delete   [X] Delete All ")
                        .alignment(Alignment::Center),
                )
                .border_style(Style::default().fg(Color::Red)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, area, &mut app.trash.state);
}

/// Renders the download tracker view
pub fn render_dl_tracker(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items: Vec<ListItem> = app
        .download
        .manager
        .queue
        .iter()
        .map(|task| {
            let prefix = match task.status {
                DownloadStatus::Pending => "⏳ ",
                DownloadStatus::Downloading => "↓ ",
                DownloadStatus::Paused => "⏸ ",
                DownloadStatus::Reconnecting => "⚠ ",
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
                DownloadStatus::Reconnecting => Color::LightRed,
            };

            let status_text = match task.status {
                DownloadStatus::Downloading => {
                    format!("[Downloading] {} {:.1}%", task.file.name, pct)
                }
                DownloadStatus::Paused => format!("[Paused] {} {:.1}%", task.file.name, pct),
                DownloadStatus::Pending => format!("[Pending] {}", task.file.name),
                DownloadStatus::Reconnecting => {
                    format!("[⚠ Reconnecting...] {} {:.1}%", task.file.name, pct)
                }
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
                    Line::from(" [Esc] Close   [x] Cancel   [p] Pause   [r] Resume ")
                        .alignment(Alignment::Center),
                )
                .border_style(Style::default().fg(app.theme_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[0], &mut app.download.manager.state);
    render_bandwidth_chart(
        f,
        chunks[1],
        &app.download.manager.speed_history,
        app.theme_color,
    );
}

/// Renders the upload tracker view
pub fn render_ul_tracker(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let items: Vec<ListItem> = app
        .upload
        .manager
        .queue
        .iter()
        .map(|task| {
            let prefix = match task.status {
                UploadStatus::Pending => "⏳ ",
                UploadStatus::Uploading => "↑ ",
                UploadStatus::Paused => "⏸ ",
                UploadStatus::Reconnecting => "⚠ ",
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
                UploadStatus::Reconnecting => Color::LightRed,
            };

            let status_text = match task.status {
                UploadStatus::Uploading => {
                    format!("[Uploading] {} {:.1}%", task.name, pct)
                }
                UploadStatus::Paused => format!("[Paused] {} {:.1}%", task.name, pct),
                UploadStatus::Pending => format!("[Pending] {}", task.name),
                UploadStatus::Reconnecting => {
                    format!("[⚠ Reconnecting...] {} {:.1}%", task.name, pct)
                }
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
                    Line::from(" [Esc] Close   [x] Cancel   [p] Pause   [r] Resume ")
                        .alignment(Alignment::Center),
                )
                .border_style(Style::default().fg(app.theme_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, chunks[0], &mut app.upload.manager.state);
    render_bandwidth_chart(
        f,
        chunks[1],
        &app.upload.manager.speed_history,
        app.theme_color,
    );
}

/// Renders the top directory path bar
fn render_path_bar(f: &mut Frame, area: Rect, app: &App) {
    let path_str = if app.nav.path_names.len() == 1 {
        "/".to_string()
    } else {
        format!("/{}", app.nav.path_names[1..].join("/"))
    };

    let top_block = Paragraph::new(format!(" Path: {} ", path_str)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" doraivu ")
            .border_style(Style::default().fg(app.theme_color)),
    );
    f.render_widget(top_block, area);
}

/// Builds the file list widget
fn render_files_list<'a>(
    files: &'a [DriveFile],
    selected_files: &HashSet<String>,
    theme_color: Color,
) -> List<'a> {
    let items: Vec<ListItem> = files
        .iter()
        .map(|file| {
            let (icon, style) =
                crate::tui::icons::get_file_meta(&file.name, &file.mime_type, theme_color);

            let mut line_spans = vec![];
            if selected_files.contains(&file.id) {
                line_spans.push(Span::styled(
                    " ● ",
                    Style::default()
                        .fg(theme_color)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            line_spans.push(Span::styled(icon, style));
            line_spans.push(Span::styled(&file.name, style.add_modifier(Modifier::BOLD)));

            ListItem::new(Line::from(line_spans))
        })
        .collect();

    List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Files ")
                .border_style(Style::default().fg(theme_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ")
}

/// Renders the preview panel (image, metadata, loading, or placeholder)
fn render_preview_panel(
    f: &mut Frame,
    area: Rect,
    preview: &mut crate::tui::state::PreviewContext,
    theme_color: Color,
) {
    match &mut preview.state {
        PreviewState::Image(ref mut protocol) => {
            let preview_block = Block::default()
                .borders(Borders::ALL)
                .title(" Preview ")
                .border_style(Style::default().fg(theme_color));
            let inner_area = preview_block.inner(area);
            f.render_widget(preview_block, area);

            f.render_stateful_widget(ratatui_image::StatefulImage::new(), inner_area, protocol);
        }
        PreviewState::Metadata {
            name,
            size,
            created,
            modified,
        } => {
            let preview_block = Block::default()
                .borders(Borders::ALL)
                .title(" Metadata ")
                .border_style(Style::default().fg(theme_color));
            let text = vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(name.as_str()),
                ]),
                Line::from(vec![
                    Span::styled("Size: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(if let Some(s) = size {
                        format_bytes(*s)
                    } else {
                        "-".to_string()
                    }),
                ]),
                Line::from(vec![
                    Span::styled("Created: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(crate::drive::api::format_time(created)),
                ]),
                Line::from(vec![
                    Span::styled("Modified: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(crate::drive::api::format_time(modified)),
                ]),
            ];
            let p = Paragraph::new(text)
                .block(preview_block)
                .wrap(Wrap { trim: true });
            f.render_widget(p, area);
        }
        PreviewState::Loading => {
            let preview_block = Block::default()
                .borders(Borders::ALL)
                .title(" Preview ")
                .border_style(Style::default().fg(theme_color));
            f.render_widget(
                Paragraph::new("Loading preview...").block(preview_block),
                area,
            );
        }
        PreviewState::None => {
            let preview_block = Block::default()
                .borders(Borders::ALL)
                .title(" Preview ")
                .border_style(Style::default().fg(theme_color));
            f.render_widget(
                Paragraph::new("No preview available.").block(preview_block),
                area,
            );
        }
    }
}

/// Renders the main content area (files list + preview panel or active tracker/trash view)
fn render_main_content(f: &mut Frame, area: Rect, app: &mut App) {
    match app.input_mode {
        InputMode::TrashView
        | InputMode::TrashDeleteConfirmModal
        | InputMode::TrashDeleteAllConfirmModal => {
            render_trash_block(f, area, app);
        }
        InputMode::DownloadTrackerView => {
            render_dl_tracker(f, area, app);
        }
        InputMode::UploadTrackerView => {
            render_ul_tracker(f, area, app);
        }
        _ => {
            let list = render_files_list(&app.files, &app.selected_files, app.theme_color);
            if app.preview.mode != PreviewMode::Hidden {
                let is_image = matches!(app.preview.state, PreviewState::Image(_));
                let center_chunks = if is_image {
                    let available_height = area.height.saturating_sub(2);
                    let preview_width = if let Some((img_w, img_h)) = app.preview.dims {
                        let font_ratio = 2.0_f64;
                        if img_h > 0 {
                            let cols = (img_w as f64 * available_height as f64 * font_ratio)
                                / (img_h as f64);
                            (cols.ceil() as u16).saturating_add(2)
                        } else {
                            (area.width / 2).max(20)
                        }
                    } else {
                        (area.width / 2).max(20)
                    };

                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Min(20), Constraint::Length(preview_width)])
                        .split(area)
                } else {
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Fill(1), Constraint::Percentage(40)])
                        .split(area)
                };

                f.render_stateful_widget(list, center_chunks[0], &mut app.state);
                render_preview_panel(f, center_chunks[1], &mut app.preview, app.theme_color);
            } else {
                f.render_stateful_widget(list, area, &mut app.state);
            }
        }
    }
}

/// Renders the bottom status and quota bar
fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let clipboard_text = if let Some(ref cb) = app.clipboard {
        let action_str = match cb.action {
            ClipboardAction::Copy => "Copy",
            ClipboardAction::Move => "Move",
        };
        let items_str = if cb.file_ids.len() == 1 {
            "item"
        } else {
            "items"
        };
        Some(format!(
            "[📋 {} {} ({})]",
            cb.file_ids.len(),
            items_str,
            action_str
        ))
    } else {
        None
    };

    let (status_area, clipboard_area, storage_area) = if let Some(ref cb_text) = clipboard_text {
        let cb_width = (cb_text.len() as u16 + 4).max(22);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(cb_width),
                Constraint::Length(35),
            ])
            .split(area);
        (chunks[0], Some((chunks[1], cb_text)), chunks[2])
    } else {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(35)])
            .split(area);
        (chunks[0], None, chunks[1])
    };

    // Left: Status / Search / Progress Gauge
    let dl_reconnecting = app
        .download
        .manager
        .queue
        .iter()
        .any(|task| task.status == DownloadStatus::Reconnecting);
    let ul_reconnecting = app
        .upload
        .manager
        .queue
        .iter()
        .any(|task| task.status == UploadStatus::Reconnecting);

    if dl_reconnecting {
        let warning_block = Paragraph::new(" [⚠ Reconnecting...] Retrying download...")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Downloading ")
                    .border_style(Style::default().fg(Color::LightRed)),
            )
            .style(
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(warning_block, status_area);
    } else if ul_reconnecting {
        let warning_block = Paragraph::new(" [⚠ Reconnecting...] Retrying upload...")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Uploading ")
                    .border_style(Style::default().fg(Color::LightRed)),
            )
            .style(
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(warning_block, status_area);
    } else if let Some((dl, total, speed)) = app.download.progress {
        let percent = if total > 0 {
            ((dl as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u16
        } else {
            0
        };
        let label = format!(
            "{} / {} - {}/s",
            format_bytes(dl),
            format_bytes(total),
            format_bytes(speed as u64)
        );
        let span = Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Downloading ")
                    .border_style(Style::default().fg(app.theme_color)),
            )
            .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
            .percent(percent)
            .label(span);
        f.render_widget(gauge, status_area);
    } else if let Some((up, total, speed)) = app.upload.progress {
        let percent = if total > 0 {
            ((up as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as u16
        } else {
            0
        };
        let label = format!(
            "{} / {} - {}/s",
            format_bytes(up),
            format_bytes(total),
            format_bytes(speed as u64)
        );
        let span = Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Uploading ")
                    .border_style(Style::default().fg(app.theme_color)),
            )
            .gauge_style(Style::default().fg(Color::Blue).bg(Color::DarkGray))
            .percent(percent)
            .label(span);
        f.render_widget(gauge, status_area);
    } else if app.search.active {
        let search_block = Paragraph::new(format!("/{}", app.search.query))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Search ")
                    .border_style(Style::default().fg(app.theme_color)),
            )
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(search_block, status_area);
    } else {
        let status_block = Paragraph::new(app.status.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Status ")
                .border_style(Style::default().fg(app.theme_color)),
        );
        f.render_widget(status_block, status_area);
    }

    // Center: Clipboard visual indicator
    if let Some((cb_area, text)) = clipboard_area {
        let cb_block = Paragraph::new(format!(" {}", text))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Clipboard ")
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(cb_block, cb_area);
    }

    // Right: Storage Quota
    if let Some((used, limit)) = app.storage_quota {
        let percent = if limit > 0 {
            ((used as f64 / limit as f64) * 100.0).clamp(0.0, 100.0) as u16
        } else {
            0
        };

        let label = format!("{} / {}", format_bytes(used), format_bytes(limit));
        let span = Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Storage ")
                    .border_style(Style::default().fg(app.theme_color)),
            )
            .gauge_style(Style::default().fg(app.theme_color).bg(Color::DarkGray))
            .percent(percent)
            .label(span);
        f.render_widget(gauge, storage_area);
    } else {
        let empty_block = Paragraph::new("Loading...").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Storage ")
                .border_style(Style::default().fg(app.theme_color)),
        );
        f.render_widget(empty_block, storage_area);
    }
}

/// Renders active modal popups (Delete, Download, Upload, Rename)
fn render_modals(f: &mut Frame, app: &App) {
    match app.input_mode {
        InputMode::TrashDeleteConfirmModal => {
            let area = centered_rect(40, 6, f.area());
            f.render_widget(Clear, area);

            let block = Block::default()
                .title(" Delete File Permanently ")
                .title_bottom(Line::from(" [y] Yes ").alignment(Alignment::Right))
                .title_bottom(Line::from(" [n] No ").alignment(Alignment::Left))
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Red));

            let name = app
                .trash
                .state
                .selected()
                .and_then(|i| app.trash.files.get(i))
                .map(|file| file.name.as_str())
                .unwrap_or_default();
            let p = Paragraph::new(format!(
                "\nAre you sure you want to permanently delete\n{}?",
                name
            ))
            .alignment(Alignment::Center)
            .block(block);

            f.render_widget(p, area);
        }
        InputMode::TrashDeleteAllConfirmModal => {
            let area = centered_rect(40, 6, f.area());
            f.render_widget(Clear, area);

            let block = Block::default()
                .title(" Empty Trash ")
                .title_bottom(Line::from(" [y] Yes ").alignment(Alignment::Right))
                .title_bottom(Line::from(" [n] No ").alignment(Alignment::Left))
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Red));

            let p = Paragraph::new(
                "\nAre you sure you want to EMPTY TRASH?\nThis action cannot be undone.",
            )
            .alignment(Alignment::Center)
            .block(block);

            f.render_widget(p, area);
        }
        InputMode::DeleteConfirmModal => {
            let area = centered_rect(40, 6, f.area());
            f.render_widget(Clear, area);

            let block = Block::default()
                .title(" Delete File ")
                .title_bottom(Line::from(" [y] Yes ").alignment(Alignment::Right))
                .title_bottom(Line::from(" [n] No ").alignment(Alignment::Left))
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Red));

            let name = app
                .selected_file()
                .map(|f| f.name.as_str())
                .unwrap_or_default();
            let p = Paragraph::new(format!("\nAre you sure you want to delete\n{}?", name))
                .alignment(Alignment::Center)
                .block(block);

            f.render_widget(p, area);
        }
        InputMode::DownloadConfirmModal => {
            let area = centered_rect(40, 6, f.area());
            f.render_widget(Clear, area);

            let block = Block::default()
                .title(" Download File ")
                .title_bottom(Line::from(" [y] Yes ").alignment(Alignment::Right))
                .title_bottom(Line::from(" [n] No ").alignment(Alignment::Left))
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Green));

            let name = app
                .selected_file()
                .map(|f| f.name.as_str())
                .unwrap_or_default();
            let p = Paragraph::new(format!("\nAre you sure you want to download\n{}?", name))
                .alignment(Alignment::Center)
                .block(block);

            f.render_widget(p, area);
        }
        InputMode::UploadModal => {
            let area = centered_rect(60, 12, f.area());
            f.render_widget(Clear, area);

            let block = Block::default()
                .title(" Upload File ")
                .title_bottom(Line::from(" [Tab] Switch ").alignment(Alignment::Left))
                .title_bottom(Line::from(" [Enter] Upload ").alignment(Alignment::Right))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme_color));
            f.render_widget(block, area);

            let modal_chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([Constraint::Length(3), Constraint::Length(3)])
                .split(area);

            let target_style = if app.upload.input_idx == 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            let local_style = if app.upload.input_idx == 1 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            let p1 = Paragraph::new(app.upload.target_id.as_str())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Target Path ")
                        .border_style(if app.upload.input_idx == 0 {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(app.theme_color)
                        }),
                )
                .style(target_style);

            let p2 = Paragraph::new(app.upload.local_path.as_str())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Local File Path ")
                        .border_style(if app.upload.input_idx == 1 {
                            Style::default().fg(Color::Yellow)
                        } else {
                            Style::default().fg(app.theme_color)
                        }),
                )
                .style(local_style);

            f.render_widget(p1, modal_chunks[0]);
            f.render_widget(p2, modal_chunks[1]);
        }
        InputMode::RenameModal => {
            let area = centered_rect(50, 3, f.area());
            f.render_widget(Clear, area);

            let popup = Paragraph::new(format!("{}█", app.rename.buffer))
                .block(
                    Block::default()
                        .title(" [ Rename ] ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                )
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(popup, area);
        }
        InputMode::NewFolderModal => {
            let area = centered_rect(50, 3, f.area());
            f.render_widget(Clear, area);

            let popup = Paragraph::new(format!("{}█", app.new_folder_buffer))
                .block(
                    Block::default()
                        .title(" [ Create New Folder ] ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                )
                .style(Style::default().fg(Color::Yellow));
            f.render_widget(popup, area);
        }
        _ => {}
    }
}

/// Renders the entire TUI application state into the given terminal frame
pub fn render(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Top block (directory)
            Constraint::Min(0),    // Main content
            Constraint::Length(3), // Status bar / Quota
        ])
        .split(f.area());

    render_path_bar(f, chunks[0], app);
    render_main_content(f, chunks[1], app);
    render_status_bar(f, chunks[2], app);
    render_modals(f, app);
}
