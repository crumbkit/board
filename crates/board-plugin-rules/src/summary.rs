//! A control's summary when its record gives none: the first sentence of
//! its requirement, cut to fit the shared `Summary` shape.

use board_plugin::shape::SUMMARY_WORDS;

/// The first sentence of `text`, outside inline code, on one line and cut to
/// [`SUMMARY_WORDS`] words, the last of them ending in `…` when it was cut.
pub(crate) fn summary(text: &str) -> String {
    let mut code = false;
    let mut end = text.len();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '`' => code = !code,
            '.' | '!' | '?'
                if !code && chars.peek().is_none_or(|(_, next)| next.is_whitespace()) =>
            {
                end = at + c.len_utf8();
                break;
            }
            _ => {}
        }
    }
    let words: Vec<&str> = text[..end].split_whitespace().collect();
    if words.len() <= SUMMARY_WORDS {
        return words.join(" ");
    }
    format!("{}…", words[..SUMMARY_WORDS].join(" "))
}

#[cfg(test)]
mod tests;
