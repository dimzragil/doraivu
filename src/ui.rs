use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame,
};

use crate::app::App;

fn format_bytes(bytes: u64) -> String {
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

fn centered_rect(
    percent_x: u16,
    fixed_height: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
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

/// Renders the entire TUI application state into the given terminal frame
pub fn render(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Top block (directory)
            Constraint::Min(0),    // Main list
            Constraint::Length(3), // Status bar / Quota
        ])
        .split(f.area());

    // Top block
    let path_str = if app.path_names.len() == 1 {
        "/".to_string()
    } else {
        format!("/{}", app.path_names[1..].join("/"))
    };

    let top_block = Paragraph::new(format!(" Path: {} ", path_str))
        .block(Block::default().borders(Borders::ALL).title(" doraivu "));
    f.render_widget(top_block, chunks[0]);

    // Main list
    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|file| {
            let icon = if file.mime_type == "application/vnd.google-apps.folder" {
                "📁"
            } else {
                "📄"
            };

            let mut line_spans = vec![];
            if app.selected_files.contains(&file.id) {
                line_spans.push(Span::styled(
                    " ● ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            line_spans.push(Span::raw(format!("{} ", icon)));
            line_spans.push(Span::styled(
                &file.name,
                Style::default().add_modifier(Modifier::BOLD),
            ));

            let content = Line::from(line_spans);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Files "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    if app.input_mode == crate::app::InputMode::TrashView
        || app.input_mode == crate::app::InputMode::TrashDeleteConfirmModal
        || app.input_mode == crate::app::InputMode::TrashDeleteAllConfirmModal
    {
        crate::trash::render_trash_block(f, chunks[1], app);

        if app.input_mode == crate::app::InputMode::TrashDeleteConfirmModal {
            let area = centered_rect(40, 6, f.area());
            use ratatui::widgets::Clear;
            f.render_widget(Clear, area);

            use ratatui::layout::Alignment;

            let block = Block::default()
                .title(" Delete File Permanently ")
                .title_bottom(Line::from(" [y] Yes ").alignment(Alignment::Right))
                .title_bottom(Line::from(" [n] No ").alignment(Alignment::Left))
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Red));

            let name = app
                .trash_state
                .selected()
                .and_then(|i| app.trashed_files.get(i))
                .map(|file| file.name.clone())
                .unwrap_or_default();
            let p = Paragraph::new(format!(
                "\nAre you sure you want to permanently delete\n{}?",
                name
            ))
            .alignment(Alignment::Center)
            .block(block);

            f.render_widget(p, area);
        } else if app.input_mode == crate::app::InputMode::TrashDeleteAllConfirmModal {
            let area = centered_rect(40, 6, f.area());
            use ratatui::widgets::Clear;
            f.render_widget(Clear, area);

            use ratatui::layout::Alignment;

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
    } else if app.input_mode == crate::app::InputMode::DownloadTrackerView {
        crate::download::render_tracker_block(f, chunks[1], app);
    } else if app.input_mode == crate::app::InputMode::UploadTrackerView {
        crate::upload::render_tracker_block(f, chunks[1], app);
    } else if app.preview_mode != crate::app::PreviewMode::Hidden {
        match &mut app.preview_state {
            crate::app::PreviewState::Image(ref mut protocol) => {
                let available_height = chunks[1].height.saturating_sub(2); // borders
                let preview_width = if let Some((img_w, img_h)) = app.preview_dims {
                    let font_ratio = 2.0_f64;
                    if img_h > 0 {
                        let cols =
                            (img_w as f64 * available_height as f64 * font_ratio) / (img_h as f64);
                        (cols.ceil() as u16).saturating_add(2)
                    } else {
                        (chunks[1].width / 2).max(20)
                    }
                } else {
                    (chunks[1].width / 2).max(20)
                };

                let center_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(20), Constraint::Length(preview_width)])
                    .split(chunks[1]);

                f.render_stateful_widget(list, center_chunks[0], &mut app.state);

                let preview_block = Block::default().borders(Borders::ALL).title(" Preview ");
                let inner_area = preview_block.inner(center_chunks[1]);
                f.render_widget(preview_block, center_chunks[1]);

                use ratatui_image::StatefulImage;
                f.render_stateful_widget(StatefulImage::new(), inner_area, protocol);
            }
            crate::app::PreviewState::Metadata {
                name,
                size,
                created,
                modified,
            } => {
                let center_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Fill(1), Constraint::Percentage(40)])
                    .split(chunks[1]);
                f.render_stateful_widget(list, center_chunks[0], &mut app.state);

                let preview_block = Block::default().borders(Borders::ALL).title(" Metadata ");
                let text = vec![
                    Line::from(vec![
                        ratatui::text::Span::styled(
                            "Name: ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::raw(name.clone()),
                    ]),
                    Line::from(vec![
                        ratatui::text::Span::styled(
                            "Size: ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::raw(if let Some(s) = size {
                            format_bytes(*s)
                        } else {
                            "-".to_string()
                        }),
                    ]),
                    Line::from(vec![
                        ratatui::text::Span::styled(
                            "Created: ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::raw(crate::api::format_time(created)),
                    ]),
                    Line::from(vec![
                        ratatui::text::Span::styled(
                            "Modified: ",
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        ratatui::text::Span::raw(crate::api::format_time(modified)),
                    ]),
                ];
                let p = Paragraph::new(text)
                    .block(preview_block)
                    .wrap(ratatui::widgets::Wrap { trim: true });
                f.render_widget(p, center_chunks[1]);
            }
            crate::app::PreviewState::Loading => {
                let center_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Fill(1), Constraint::Percentage(40)])
                    .split(chunks[1]);
                f.render_stateful_widget(list, center_chunks[0], &mut app.state);
                let preview_block = Block::default().borders(Borders::ALL).title(" Preview ");
                f.render_widget(
                    Paragraph::new("Loading preview...").block(preview_block),
                    center_chunks[1],
                );
            }
            crate::app::PreviewState::None => {
                let center_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Fill(1), Constraint::Percentage(40)])
                    .split(chunks[1]);
                f.render_stateful_widget(list, center_chunks[0], &mut app.state);
                let preview_block = Block::default().borders(Borders::ALL).title(" Preview ");
                f.render_widget(
                    Paragraph::new("No preview available.").block(preview_block),
                    center_chunks[1],
                );
            }
        }
    } else {
        f.render_stateful_widget(list, chunks[1], &mut app.state);
    }

    // Bottom block: Status/Search and Quota
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(35), // Fixed width for quota
        ])
        .split(chunks[2]);

    // Status / Search
    if let Some((dl, total, speed)) = app.download_progress {
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
        let span = ratatui::text::Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Downloading "),
            )
            .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
            .percent(percent)
            .label(span);
        f.render_widget(gauge, bottom_chunks[0]);
    } else if let Some((up, total, speed)) = app.upload_progress {
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
        let span = ratatui::text::Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" Uploading "))
            .gauge_style(Style::default().fg(Color::Blue).bg(Color::DarkGray))
            .percent(percent)
            .label(span);
        f.render_widget(gauge, bottom_chunks[0]);
    } else if app.search_mode {
        let search_block = Paragraph::new(format!("/{}", app.search_query))
            .block(Block::default().borders(Borders::ALL).title(" Search "))
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(search_block, bottom_chunks[0]);
    } else {
        let status_block = Paragraph::new(app.status.clone())
            .block(Block::default().borders(Borders::ALL).title(" Status "));
        f.render_widget(status_block, bottom_chunks[0]);
    }

    // Quota
    if let Some((used, limit)) = app.storage_quota {
        let percent = if limit > 0 {
            ((used as f64 / limit as f64) * 100.0).clamp(0.0, 100.0) as u16
        } else {
            0
        };

        let label = format!("{} / {}", format_bytes(used), format_bytes(limit));

        let span = ratatui::text::Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" Storage "))
            .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            .percent(percent)
            .label(span);
        f.render_widget(gauge, bottom_chunks[1]);
    } else {
        let empty_block = Paragraph::new("Loading...")
            .block(Block::default().borders(Borders::ALL).title(" Storage "));
        f.render_widget(empty_block, bottom_chunks[1]);
    }

    if app.input_mode == crate::app::InputMode::DeleteConfirmModal {
        let area = centered_rect(40, 6, f.area());
        use ratatui::widgets::Clear;
        f.render_widget(Clear, area);

        use ratatui::layout::Alignment;

        let block = Block::default()
            .title(" Delete File ")
            .title_bottom(Line::from(" [y] Yes ").alignment(Alignment::Right))
            .title_bottom(Line::from(" [n] No ").alignment(Alignment::Left))
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Red));

        let name = app
            .selected_file()
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let p = Paragraph::new(format!("\nAre you sure you want to delete\n{}?", name))
            .alignment(Alignment::Center)
            .block(block);

        f.render_widget(p, area);
    } else if app.input_mode == crate::app::InputMode::DownloadConfirmModal {
        let area = centered_rect(40, 6, f.area());
        use ratatui::widgets::Clear;
        f.render_widget(Clear, area);

        use ratatui::layout::Alignment;

        let block = Block::default()
            .title(" Download File ")
            .title_bottom(Line::from(" [y] Yes ").alignment(Alignment::Right))
            .title_bottom(Line::from(" [n] No ").alignment(Alignment::Left))
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Green));

        let name = app
            .selected_file()
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let p = Paragraph::new(format!("\nAre you sure you want to download\n{}?", name))
            .alignment(Alignment::Center)
            .block(block);

        f.render_widget(p, area);
    } else if app.input_mode == crate::app::InputMode::UploadModal {
        let area = centered_rect(60, 12, f.area()); // 12 lines fixed height
        use ratatui::widgets::Clear;
        f.render_widget(Clear, area);

        use ratatui::layout::Alignment;

        let block = Block::default()
            .title(" Upload File ")
            .title_bottom(Line::from(" [Tab] Switch ").alignment(Alignment::Left))
            .title_bottom(Line::from(" [Enter] Upload ").alignment(Alignment::Right))
            .borders(Borders::ALL);
        f.render_widget(block, area);

        let modal_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([Constraint::Length(3), Constraint::Length(3)])
            .split(area);

        let target_style = if app.upload_input_idx == 0 {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let local_style = if app.upload_input_idx == 1 {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let p1 = Paragraph::new(app.upload_target_id.clone())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Target Path "),
            )
            .style(target_style);

        let p2 = Paragraph::new(app.upload_local_path.clone())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Local File Path "),
            )
            .style(local_style);

        f.render_widget(p1, modal_chunks[0]);
        f.render_widget(p2, modal_chunks[1]);
    }
}
