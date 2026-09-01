use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Gauge},
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

            let content = Line::from(vec![
                Span::raw(format!("{} ", icon)),
                Span::styled(&file.name, Style::default().add_modifier(Modifier::BOLD)),
            ]);
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

    if app.show_preview {
        let available_height = chunks[1].height.saturating_sub(2); // borders
        
        let preview_width = if let Some((img_w, img_h)) = app.preview_dims {
            // Terminal cells universally have a roughly 1:2 aspect ratio (width:height).
            // The exact pixel size doesn't matter, only the ratio for calculating column width.
            let font_ratio = 2.0_f64; // height / width
            
            if img_h > 0 {
                let cols = (img_w as f64 * available_height as f64 * font_ratio) / (img_h as f64);
                (cols.ceil() as u16).saturating_add(2) // +2 for borders
            } else {
                40
            }
        } else {
            40
        };

        let center_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(preview_width)])
            .split(chunks[1]);
            
        f.render_stateful_widget(list, center_chunks[0], &mut app.state);
        
        let preview_block = Block::default().borders(Borders::ALL).title(" Preview ");
        let inner_area = preview_block.inner(center_chunks[1]);
        f.render_widget(preview_block, center_chunks[1]);
        
        if let Some(ref mut protocol) = app.preview_image {
            use ratatui_image::StatefulImage;
            f.render_stateful_widget(StatefulImage::new(), inner_area, protocol);
        } else {
            f.render_widget(Paragraph::new("Loading or unavailable..."), inner_area);
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
        let gauge = Gauge::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Downloading "),
            )
            .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
            .percent(percent)
            .label(label);
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
        
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(" Storage "))
            .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            .percent(percent)
            .label(label);
        f.render_widget(gauge, bottom_chunks[1]);
    } else {
        let empty_block = Paragraph::new("Loading...")
            .block(Block::default().borders(Borders::ALL).title(" Storage "));
        f.render_widget(empty_block, bottom_chunks[1]);
    }
}
