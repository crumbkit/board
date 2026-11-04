//! The file name Reflect gives a note of a title, which notes and tasks
//! name new files by.

use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

/// Reflect's `slugForTitle` (packages/core/src/markdown/slug.ts), whose
/// golden corpus freezes it, caps a slug at this many characters.
const SLUG_CHARS: usize = 60;
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Whether `c` separates words: JavaScript's `\s`, which differs from
/// Rust's `char::is_whitespace` (it holds U+FEFF, and not U+001C to U+001F
/// or U+0085), or `_` or `-`.
fn separator(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
                | '_'
                | '-'
    )
}

/// Whether `c` is a letter or a number.
fn word(c: char) -> bool {
    use GeneralCategory::{
        DecimalNumber, LetterNumber, LowercaseLetter, ModifierLetter, OtherLetter, OtherNumber,
        TitlecaseLetter, UppercaseLetter,
    };
    matches!(
        get_general_category(c),
        UppercaseLetter
            | LowercaseLetter
            | TitlecaseLetter
            | ModifierLetter
            | OtherLetter
            | DecimalNumber
            | LetterNumber
            | OtherNumber
    )
}

/// The file name Reflect gives a note titled `title`, without `.md`. It
/// drops a letter or number newer than this crate's Unicode tables know,
/// where Reflect's JavaScript, on newer tables, keeps it.
pub fn slug(title: &str) -> String {
    let folded = title.nfc().collect::<String>().to_lowercase();
    let mut joined = String::new();
    for c in folded.chars().filter(|&c| word(c) || separator(c)) {
        if !separator(c) {
            joined.push(c);
        } else if !joined.ends_with('-') {
            joined.push('-');
        }
    }
    let capped: String = joined.trim_matches('-').chars().take(SLUG_CHARS).collect();
    let capped = capped.trim_matches('-');
    if capped.is_empty() {
        "untitled".to_owned()
    } else if RESERVED.contains(&capped) {
        format!("{capped}-note")
    } else {
        capped.to_owned()
    }
}

#[cfg(test)]
mod tests;
