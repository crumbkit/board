//! The app's names, and nothing else. Renaming the app changes this file
//! alone: the `board::` codes, the reserved `board` type namespace, the crate
//! names and the repository name are separate decisions that do not follow it.

/// The name people read, in prose and messages.
pub const DISPLAY_NAME: &str = "Board";

/// The installed executable, and the name a command line starts with.
pub const BINARY_NAME: &str = "board";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_name_is_one_lowercase_word_a_shell_can_run() {
        assert_ne!(BINARY_NAME, "");
        assert!(
            BINARY_NAME
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        );
    }

    #[test]
    fn the_display_name_is_a_name_without_surrounding_space() {
        assert_ne!(DISPLAY_NAME, "");
        assert_eq!(DISPLAY_NAME.trim(), DISPLAY_NAME);
    }
}
