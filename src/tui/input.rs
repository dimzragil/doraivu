use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Finds the previous word boundary index before `cur` in `chars`.
pub fn prev_word_boundary(chars: &[char], cur: usize) -> usize {
    if cur == 0 || chars.is_empty() {
        return 0;
    }
    let mut i = cur.min(chars.len());
    // skip immediate separators (e.g. '/', ' ', '-', '_', '.')
    while i > 0
        && (chars[i - 1] == '/'
            || chars[i - 1].is_whitespace()
            || chars[i - 1] == '-'
            || chars[i - 1] == '_')
    {
        i -= 1;
    }
    // skip non-separators
    while i > 0
        && chars[i - 1] != '/'
        && !chars[i - 1].is_whitespace()
        && chars[i - 1] != '-'
        && chars[i - 1] != '_'
    {
        i -= 1;
    }
    i
}

/// Finds the next word boundary index after `cur` in `chars`.
pub fn next_word_boundary(chars: &[char], cur: usize) -> usize {
    let len = chars.len();
    if cur >= len {
        return len;
    }
    let mut i = cur;
    // skip non-separators
    while i < len
        && chars[i] != '/'
        && !chars[i].is_whitespace()
        && chars[i] != '-'
        && chars[i] != '_'
    {
        i += 1;
    }
    // skip separators
    while i < len
        && (chars[i] == '/' || chars[i].is_whitespace() || chars[i] == '-' || chars[i] == '_')
    {
        i += 1;
    }
    i
}

/// Handles a key event on an editable text buffer with cursor.
/// Returns `true` if text or cursor position was modified.
pub fn handle_input_key(text: &mut String, cursor: &mut usize, key: KeyEvent) -> bool {
    let mut chars: Vec<char> = text.chars().collect();
    let mut cur = (*cursor).min(chars.len());
    let mut changed = false;

    let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let is_alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Left => {
            if is_ctrl || is_alt {
                cur = prev_word_boundary(&chars, cur);
            } else {
                cur = cur.saturating_sub(1);
            }
            changed = true;
        }
        KeyCode::Right => {
            if is_ctrl || is_alt {
                cur = next_word_boundary(&chars, cur);
            } else if cur < chars.len() {
                cur += 1;
            }
            changed = true;
        }
        KeyCode::Home => {
            cur = 0;
            changed = true;
        }
        KeyCode::End => {
            cur = chars.len();
            changed = true;
        }
        KeyCode::Backspace => {
            if cur > 0 && cur <= chars.len() {
                chars.remove(cur - 1);
                cur -= 1;
                *text = chars.iter().collect();
                changed = true;
            }
        }
        KeyCode::Delete => {
            if cur < chars.len() {
                chars.remove(cur);
                *text = chars.iter().collect();
                changed = true;
            }
        }
        KeyCode::Char(c) if is_ctrl => match c {
            'a' => {
                cur = 0;
                changed = true;
            }
            'e' => {
                cur = chars.len();
                changed = true;
            }
            'h' => {
                if cur > 0 && cur <= chars.len() {
                    chars.remove(cur - 1);
                    cur -= 1;
                    *text = chars.iter().collect();
                    changed = true;
                }
            }
            'd' => {
                if cur < chars.len() {
                    chars.remove(cur);
                    *text = chars.iter().collect();
                    changed = true;
                }
            }
            'w' => {
                let prev = prev_word_boundary(&chars, cur);
                if prev < cur {
                    chars.drain(prev..cur);
                    cur = prev;
                    *text = chars.iter().collect();
                    changed = true;
                }
            }
            'u' if cur > 0 => {
                chars.drain(0..cur);
                cur = 0;
                *text = chars.iter().collect();
                changed = true;
            }
            'k' if cur < chars.len() => {
                chars.truncate(cur);
                *text = chars.iter().collect();
                changed = true;
            }
            _ => {}
        },
        KeyCode::Char(c) if !is_ctrl && !is_alt => {
            chars.insert(cur, c);
            cur += 1;
            *text = chars.iter().collect();
            changed = true;
        }
        _ => {}
    }

    *cursor = cur.min(text.chars().count());
    changed
}

/// Computes the visible portion of `text` and the screen-relative cursor column
/// within an input box of width `width` characters.
pub fn compute_visible_input(text: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());

    let scroll = if cursor < width {
        0
    } else {
        (cursor + 1).saturating_sub(width)
    };

    let end = (scroll + width).min(chars.len());
    let visible_slice: String = chars[scroll..end].iter().collect();
    let cursor_col = cursor.saturating_sub(scroll).min(width.saturating_sub(1));

    (visible_slice, cursor_col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn test_input_cursor_left_right() {
        let mut text = "hello".to_string();
        let mut cursor = 5;

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Left));
        assert_eq!(cursor, 4);

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Left));
        assert_eq!(cursor, 3);

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Right));
        assert_eq!(cursor, 4);

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Home));
        assert_eq!(cursor, 0);

        handle_input_key(&mut text, &mut cursor, key(KeyCode::End));
        assert_eq!(cursor, 5);
    }

    #[test]
    fn test_input_insert_middle() {
        let mut text = "hllo".to_string();
        let mut cursor = 1;

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Char('e')));
        assert_eq!(text, "hello");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn test_input_backspace_middle() {
        let mut text = "heeello".to_string();
        let mut cursor = 3;

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Backspace));
        assert_eq!(text, "heello");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn test_input_delete_middle() {
        let mut text = "hexxllo".to_string();
        let mut cursor = 2;

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Delete));
        assert_eq!(text, "hexllo");
        assert_eq!(cursor, 2);

        handle_input_key(&mut text, &mut cursor, key(KeyCode::Delete));
        assert_eq!(text, "hello");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn test_word_boundary_and_ctrl_w() {
        let mut text = "/home/user/document.pdf".to_string();
        let mut cursor = text.len();

        handle_input_key(&mut text, &mut cursor, ctrl_key('w'));
        assert_eq!(text, "/home/user/");
        assert_eq!(cursor, 11);

        handle_input_key(&mut text, &mut cursor, ctrl_key('w'));
        assert_eq!(text, "/home/");
        assert_eq!(cursor, 6);
    }

    #[test]
    fn test_compute_visible_input_scrolling() {
        let text = "0123456789ABCDEF";
        let width = 10;

        let (visible, col) = compute_visible_input(text, 0, width);
        assert_eq!(visible, "0123456789");
        assert_eq!(col, 0);

        let (visible, col) = compute_visible_input(text, 5, width);
        assert_eq!(visible, "0123456789");
        assert_eq!(col, 5);

        let (visible, col) = compute_visible_input(text, 16, width);
        assert_eq!(visible, "789ABCDEF");
        assert_eq!(col, 9);
    }
}
