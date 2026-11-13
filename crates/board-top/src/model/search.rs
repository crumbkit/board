//! The `/` search: a filter on the rows and the prompt that edits it. The
//! event loop passes keys; every decision is here.

use super::{Group, Row, View};

impl View {
    /// Whether a row passes the filter: the query, ignoring case, is in the
    /// project, the title or the change path. An empty query passes every row.
    #[must_use]
    pub fn passes(&self, group: &Group, row: &Row) -> bool {
        if self.filter.is_empty() {
            return true;
        }
        let needle = self.filter.to_lowercase();
        [
            group.project.as_str(),
            row.title.as_str(),
            row.path.as_str(),
        ]
        .iter()
        .any(|text| text.to_lowercase().contains(&needle))
    }

    /// Whether the group shows: always without a filter, otherwise when one
    /// of its rows passes. Its heading and ended line follow the same rule.
    #[must_use]
    pub fn group_shows(&self, group: &Group) -> bool {
        self.filter.is_empty() || group.rows.iter().any(|row| self.passes(group, row))
    }

    /// The number of rows before the filter.
    #[must_use]
    pub fn unfiltered_rows(&self) -> usize {
        self.groups.iter().map(|group| group.rows.len()).sum()
    }

    /// Opens the prompt, keeping any query so it can be edited.
    pub fn start_search(&mut self) {
        self.typing = true;
    }

    /// Adds a character to the query.
    pub fn type_char(&mut self, c: char) {
        self.edit(|view| view.filter.push(c));
    }

    /// Removes the last character of the query.
    pub fn backspace(&mut self) {
        self.edit(|view| {
            view.filter.pop();
        });
    }

    /// Keeps the query and returns the keys to scrolling.
    pub fn keep_search(&mut self) {
        self.typing = false;
    }

    /// Clears the query, whether or not the prompt is open.
    pub fn clear_search(&mut self) {
        self.edit(|view| {
            view.filter.clear();
            view.typing = false;
        });
    }

    /// Changes the query and keeps the highlight on its record when that
    /// record still passes, otherwise on the first row.
    fn edit(&mut self, change: impl FnOnce(&mut Self)) {
        let path = self.selected_path().map(str::to_owned);
        change(self);
        self.selected = path
            .and_then(|path| self.rows().position(|row| row.path == path))
            .unwrap_or(0);
        self.scroll = 0;
    }
}
