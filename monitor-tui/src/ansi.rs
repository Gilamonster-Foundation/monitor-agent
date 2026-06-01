/// Minimal SGR (ANSI colour) → ratatui Text converter.
///
/// Handles the exact sequences chafa emits:
///   `\x1b[0m`                          reset
///   `\x1b[7m`                          reverse video
///   `\x1b[38;2;R;G;Bm`                 24-bit foreground
///   `\x1b[48;2;R;G;Bm`                 24-bit background
///   `\x1b[38;2;R;G;B;48;2;R;G;Bm`     fg + bg in one sequence
///   `\x1b[?25l` and other non-SGR codes are silently skipped.
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

pub fn ansi_to_text(src: &str) -> Text<'static> {
    let lines: Vec<Line<'static>> = src.lines().map(parse_line).collect();
    Text::from(lines)
}

fn parse_line(line: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut buf = String::new();
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Consume the '[' that always follows ESC in CSI sequences.
            if chars.peek() == Some(&'[') {
                chars.next();
            } else {
                buf.push(ch);
                continue;
            }

            // Collect everything up to and including the terminating letter.
            let mut seq = String::new();
            for c in chars.by_ref() {
                seq.push(c);
                if c.is_ascii_alphabetic() {
                    break;
                }
            }

            // Only handle SGR sequences (end with 'm').
            if seq.ends_with('m') {
                // Flush buffered text with the current style before changing it.
                if !buf.is_empty() {
                    spans.push(Span::styled(buf.clone(), style));
                    buf.clear();
                }
                let params = &seq[..seq.len() - 1]; // strip trailing 'm'
                style = apply_sgr(params, style);
            }
            // Non-SGR sequences (e.g. `?25l`) are silently dropped.
        } else {
            buf.push(ch);
        }
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, style));
    }
    Line::from(spans)
}

/// Apply one SGR parameter string (the part between `\x1b[` and `m`) to `style`.
fn apply_sgr(params: &str, mut style: Style) -> Style {
    // Parse the flat numeric list, e.g. "38;2;120;34;56;48;2;0;0;0".
    let nums: Vec<u16> = params.split(';').filter_map(|s| s.parse().ok()).collect();

    let mut i = 0;
    while i < nums.len() {
        match nums[i] {
            0 => {
                style = Style::default();
            }
            1 => {
                style = style.add_modifier(Modifier::BOLD);
            }
            7 => {
                style = style.add_modifier(Modifier::REVERSED);
            }
            38 if i + 4 < nums.len() && nums[i + 1] == 2 => {
                style = style.fg(Color::Rgb(
                    nums[i + 2] as u8,
                    nums[i + 3] as u8,
                    nums[i + 4] as u8,
                ));
                i += 4;
            }
            48 if i + 4 < nums.len() && nums[i + 1] == 2 => {
                style = style.bg(Color::Rgb(
                    nums[i + 2] as u8,
                    nums[i + 3] as u8,
                    nums[i + 4] as u8,
                ));
                i += 4;
            }
            _ => {}
        }
        i += 1;
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_clears_style() {
        let result = apply_sgr("0", Style::default().fg(Color::Red));
        assert_eq!(result, Style::default());
    }

    #[test]
    fn foreground_24bit() {
        let s = apply_sgr("38;2;255;128;0", Style::default());
        assert_eq!(s.fg, Some(Color::Rgb(255, 128, 0)));
    }

    #[test]
    fn background_24bit() {
        let s = apply_sgr("48;2;10;20;30", Style::default());
        assert_eq!(s.bg, Some(Color::Rgb(10, 20, 30)));
    }

    #[test]
    fn compound_fg_bg() {
        let s = apply_sgr("38;2;100;150;200;48;2;10;20;30", Style::default());
        assert_eq!(s.fg, Some(Color::Rgb(100, 150, 200)));
        assert_eq!(s.bg, Some(Color::Rgb(10, 20, 30)));
    }

    #[test]
    fn reverse_modifier() {
        let s = apply_sgr("7", Style::default());
        assert!(s.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn non_sgr_sequence_produces_no_crash() {
        // `?25l` (hide cursor) should be ignored, not panic.
        let text = ansi_to_text("\x1b[?25lhello");
        let line = &text.lines[0];
        let combined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(combined, "hello");
    }

    #[test]
    fn plain_text_passthrough() {
        let text = ansi_to_text("hello world");
        assert_eq!(text.lines.len(), 1);
        let combined: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(combined, "hello world");
    }

    #[test]
    fn multiline_produces_multiple_lines() {
        let text = ansi_to_text("line1\nline2\nline3");
        assert_eq!(text.lines.len(), 3);
    }

    #[test]
    fn ansi_art_parses_without_panic() {
        // Run the full 20-col Monty art through the parser.
        let art = include_str!("../../docs/logos/monty-ansi-20.txt");
        let text = ansi_to_text(art);
        assert!(!text.lines.is_empty());
    }
}
