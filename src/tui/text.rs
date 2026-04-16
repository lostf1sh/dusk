use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    if display_width(text) <= max_width {
        return text.to_string();
    }

    if max_width <= 2 {
        return fit_to_width(text, max_width);
    }

    let mut truncated = fit_to_width(text, max_width - 2);
    truncated.push_str("..");
    truncated
}

pub fn fit_to_width(text: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut output = String::new();

    for ch in text.chars() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + char_width > max_width {
            break;
        }

        output.push(ch);
        width += char_width;
    }

    output
}
