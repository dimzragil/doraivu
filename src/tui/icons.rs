use ratatui::style::{Color, Modifier, Style};

/// Returns a Nerd Font icon string and a matching Ratatui `Style`
/// based on file name, extension, MIME type, and current TUI theme color.
pub fn get_file_meta(name: &str, mime_type: &str, theme_color: Color) -> (String, Style) {
    let lower_name = name.to_lowercase();

    // 1. Folders
    if mime_type == "application/vnd.google-apps.folder"
        || mime_type.ends_with("folder")
        || lower_name == "my drive"
        || lower_name == "shared with me"
    {
        return (
            "\u{f07b} ".to_string(), //  nf-fa-folder
            Style::default()
                .fg(theme_color)
                .add_modifier(Modifier::BOLD),
        );
    }

    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    // 2. Archives
    if lower_name.ends_with(".tar.gz")
        || lower_name.ends_with(".tar.bz2")
        || lower_name.ends_with(".tar.xz")
        || matches!(
            ext.as_str(),
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "tgz"
        )
        || matches!(
            mime_type,
            "application/zip"
                | "application/x-tar"
                | "application/gzip"
                | "application/x-7z-compressed"
                | "application/x-rar-compressed"
        )
    {
        return (
            "\u{f410} ".to_string(), //  nf-oct-file_zip
            Style::default().fg(Color::LightYellow),
        );
    }

    // 3. Media (Video, Image, Audio)
    if mime_type.starts_with("video/")
        || matches!(
            ext.as_str(),
            "mp4" | "mkv" | "avi" | "mov" | "flv" | "webm" | "wmv" | "m4v"
        )
    {
        return (
            "\u{f03d} ".to_string(), //  nf-fa-video_camera
            Style::default().fg(Color::Magenta),
        );
    }

    if mime_type.starts_with("image/")
        || matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" | "tiff"
        )
    {
        return (
            "\u{f1c5} ".to_string(), //  nf-fa-file_image_o
            Style::default().fg(Color::LightMagenta),
        );
    }

    if mime_type.starts_with("audio/")
        || matches!(
            ext.as_str(),
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "wma"
        )
    {
        return (
            "\u{f001} ".to_string(), //  nf-fa-music
            Style::default().fg(Color::LightCyan),
        );
    }

    // 4. Google Workspace / Office Documents
    if mime_type == "application/vnd.google-apps.document"
        || matches!(
            mime_type,
            "application/msword"
                | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        )
        || matches!(ext.as_str(), "doc" | "docx" | "odt" | "rtf")
    {
        return (
            "\u{f1c2} ".to_string(), //  nf-fa-file_word_o
            Style::default().fg(Color::Blue),
        );
    }

    if mime_type == "application/vnd.google-apps.spreadsheet"
        || matches!(
            mime_type,
            "application/vnd.ms-excel"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        )
        || matches!(ext.as_str(), "xls" | "xlsx" | "ods" | "csv" | "tsv")
    {
        return (
            "\u{f1c3} ".to_string(), //  nf-fa-file_excel_o
            Style::default().fg(Color::Green),
        );
    }

    if mime_type == "application/vnd.google-apps.presentation"
        || matches!(
            mime_type,
            "application/vnd.ms-powerpoint"
                | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        )
        || matches!(ext.as_str(), "ppt" | "pptx" | "odp")
    {
        return (
            "\u{f1c4} ".to_string(), //  nf-fa-file_powerpoint_o
            Style::default().fg(Color::Yellow),
        );
    }

    if mime_type == "application/pdf" || ext == "pdf" {
        return (
            "\u{f1c1} ".to_string(), //  nf-fa-file_pdf_o
            Style::default().fg(Color::Red),
        );
    }

    // 5. Programming / Source Code
    match ext.as_str() {
        "rs" => {
            return (
                "\u{e7a8} ".to_string(), //  nf-dev-rust
                Style::default().fg(Color::LightRed),
            );
        }
        "py" | "pyw" | "ipynb" => {
            return (
                "\u{e73c} ".to_string(), //  nf-dev-python
                Style::default().fg(Color::LightYellow),
            );
        }
        "js" | "mjs" | "cjs" | "jsx" => {
            return (
                "\u{e74e} ".to_string(), //  nf-dev-javascript
                Style::default().fg(Color::Yellow),
            );
        }
        "ts" | "tsx" => {
            return (
                "\u{e628} ".to_string(), //  nf-custom-typescript
                Style::default().fg(Color::LightBlue),
            );
        }
        "json" => {
            return (
                "\u{e60b} ".to_string(), //  nf-custom-json
                Style::default().fg(Color::LightGreen),
            );
        }
        "toml" | "yaml" | "yml" | "ini" | "conf" | "config" => {
            return (
                "\u{e615} ".to_string(), //  nf-custom-settings
                Style::default().fg(Color::LightGreen),
            );
        }
        "c" | "h" => {
            return (
                "\u{e61e} ".to_string(), //  nf-custom-c
                Style::default().fg(Color::Blue),
            );
        }
        "cpp" | "hpp" | "cc" | "cxx" => {
            return (
                "\u{e61d} ".to_string(), //  nf-custom-cpp
                Style::default().fg(Color::LightBlue),
            );
        }
        "go" => {
            return (
                "\u{e627} ".to_string(), //  nf-custom-go
                Style::default().fg(Color::Cyan),
            );
        }
        "html" | "htm" => {
            return (
                "\u{e736} ".to_string(), //  nf-dev-html5
                Style::default().fg(Color::LightRed),
            );
        }
        "css" | "scss" | "sass" | "less" => {
            return (
                "\u{e749} ".to_string(), //  nf-dev-css3
                Style::default().fg(Color::Blue),
            );
        }
        "sh" | "bash" | "zsh" | "fish" => {
            return (
                "\u{f489} ".to_string(), //  nf-oct-terminal
                Style::default().fg(Color::Green),
            );
        }
        "md" | "markdown" => {
            return (
                "\u{f48a} ".to_string(), //  nf-oct-markdown
                Style::default().fg(theme_color),
            );
        }
        _ => {}
    }

    // 6. Text / Logs
    if mime_type.starts_with("text/") || matches!(ext.as_str(), "txt" | "log") {
        return (
            "\u{f15c} ".to_string(), //  nf-fa-file_text
            Style::default().fg(Color::White),
        );
    }

    // 7. Default / Unknown
    (
        "\u{f15b} ".to_string(), //  nf-fa-file
        Style::default().fg(Color::Gray),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_folder_meta() {
        let (icon, style) =
            get_file_meta("Photos", "application/vnd.google-apps.folder", Color::Cyan);
        assert_eq!(icon, "\u{f07b} ");
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_media_meta() {
        let (icon, _) = get_file_meta("video.mp4", "video/mp4", Color::Cyan);
        assert_eq!(icon, "\u{f03d} ");

        let (icon, _) = get_file_meta("photo.jpg", "image/jpeg", Color::Cyan);
        assert_eq!(icon, "\u{f1c5} ");

        let (icon, _) = get_file_meta("song.mp3", "audio/mpeg", Color::Cyan);
        assert_eq!(icon, "\u{f001} ");
    }

    #[test]
    fn test_programming_meta() {
        let (icon, style) = get_file_meta("main.rs", "text/x-rust", Color::Cyan);
        assert_eq!(icon, "\u{e7a8} ");
        assert_eq!(style.fg, Some(Color::LightRed));

        let (icon, _) = get_file_meta("script.py", "text/x-python", Color::Cyan);
        assert_eq!(icon, "\u{e73c} ");

        let (icon, _) = get_file_meta("config.toml", "application/toml", Color::Cyan);
        assert_eq!(icon, "\u{e615} ");
    }

    #[test]
    fn test_workspace_meta() {
        let (icon, style) =
            get_file_meta("Doc", "application/vnd.google-apps.document", Color::Cyan);
        assert_eq!(icon, "\u{f1c2} ");
        assert_eq!(style.fg, Some(Color::Blue));

        let (icon, style) = get_file_meta(
            "Sheet",
            "application/vnd.google-apps.spreadsheet",
            Color::Cyan,
        );
        assert_eq!(icon, "\u{f1c3} ");
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn test_archive_and_default() {
        let (icon, _) = get_file_meta("backup.tar.gz", "application/gzip", Color::Cyan);
        assert_eq!(icon, "\u{f410} ");

        let (icon, style) = get_file_meta("unknown.xyz", "application/octet-stream", Color::Cyan);
        assert_eq!(icon, "\u{f15b} ");
        assert_eq!(style.fg, Some(Color::Gray));
    }
}
