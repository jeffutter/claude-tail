use unicode_width::UnicodeWidthStr;

/// Wrap text to fit within a given width, breaking at word boundaries.
/// Returns a vector of wrapped lines.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    for line in text.lines() {
        if line.width() <= width {
            lines.push(line.to_string());
        } else {
            // Simple word wrapping
            let mut current_line = String::new();
            for word in line.split_whitespace() {
                if current_line.is_empty() {
                    current_line = word.to_string();
                } else if current_line.width() + 1 + word.width() <= width {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    lines.push(current_line);
                    current_line = word.to_string();
                }
            }
            if !current_line.is_empty() {
                lines.push(current_line);
            }
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Count how many lines are needed to wrap text, using word-aware wrapping.
/// This must match the count produced by wrap_text().len() for consistency.
pub fn wrap_text_line_count(text: &str, width: usize) -> usize {
    wrap_text(text, width).len()
}
