//! Which lines of the body are on screen, and how the keys move the highlight.
//! The event loop only passes keys and the height; every decision is here.

use super::View;

/// One line of the body: what the renderer draws, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyLine {
    /// A realm heading.
    Realm(String),
    /// A row: the group, the position in the group and the index among all rows.
    Row {
        /// The group's index.
        group: usize,
        /// The position in the group.
        offset: usize,
        /// Whether it is the first row the group shows.
        first: bool,
        /// The index among all rows.
        index: usize,
    },
    /// A group's "+N landed, M dropped" line.
    Ended(usize),
}

/// The part of the body to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// The first line drawn.
    pub start: usize,
    /// One past the last line drawn.
    pub end: usize,
    /// The rows among those lines, as the one-based first and last row, or
    /// `None` when no row is drawn.
    pub rows: Option<(usize, usize)>,
    /// The number of rows that pass the filter.
    pub total: usize,
    /// The number of rows before the filter.
    pub unfiltered: usize,
    /// Whether the whole body is drawn, so no position line is needed. A
    /// filter always needs the line.
    pub whole: bool,
}

impl View {
    /// Every line of the body, in order.
    #[must_use]
    pub fn body(&self) -> Vec<BodyLine> {
        let mut lines = Vec::new();
        let mut index = 0;
        let mut last_realm: Option<&str> = None;
        for (at, group) in self.groups.iter().enumerate() {
            if !self.group_shows(group) {
                continue;
            }
            if self.mapped && last_realm != group.realm.as_deref() {
                if let Some(realm) = &group.realm {
                    lines.push(BodyLine::Realm(realm.clone()));
                }
                last_realm = group.realm.as_deref();
            }
            let mut first = true;
            for (offset, row) in group.rows.iter().enumerate() {
                if !self.passes(group, row) {
                    continue;
                }
                lines.push(BodyLine::Row {
                    group: at,
                    offset,
                    first,
                    index,
                });
                first = false;
                index += 1;
            }
            if group.has_ended() {
                lines.push(BodyLine::Ended(at));
            }
        }
        lines
    }

    /// The lines to draw in `capacity` lines: all of them when they fit,
    /// otherwise one line fewer, which the position line takes. The window
    /// keeps the highlighted row in view.
    #[must_use]
    pub fn window(&self, capacity: usize) -> Window {
        let body = self.body();
        let total = self.rows().count();
        let whole = body.len() <= capacity && self.filter.is_empty();
        let room = if whole {
            capacity
        } else {
            capacity.saturating_sub(1).max(1)
        };
        let selected = body.iter().position(
            |line| matches!(line, BodyLine::Row { index, .. } if *index == self.selected),
        );
        let mut start = self.scroll.min(body.len().saturating_sub(room));
        let at_bottom = self.scroll == usize::MAX;
        if at_bottom {
            if let Some(at) = selected {
                start = start.min(at);
            }
        } else if let Some(at) = selected {
            if at < start {
                start = at;
                while start > 0 && matches!(body[start - 1], BodyLine::Realm(_)) {
                    start -= 1;
                }
            } else if at >= start + room {
                start = at + 1 - room;
            }
        }
        let end = (start + room).min(body.len());
        let shown: Vec<usize> = body[start..end]
            .iter()
            .filter_map(|line| match line {
                BodyLine::Row { index, .. } => Some(*index + 1),
                _ => None,
            })
            .collect();
        let rows = shown.first().zip(shown.last()).map(|(a, b)| (*a, *b));
        Window {
            start,
            end,
            rows,
            total,
            unfiltered: self.unfiltered_rows(),
            whole,
        }
    }

    /// Remembers the window for `capacity`, so a later change in the list
    /// starts from where the screen was.
    pub fn reveal(&mut self, capacity: usize) {
        self.shown = self.window(capacity).start;
        if self.scroll != usize::MAX {
            self.scroll = self.shown;
        }
    }

    /// Moves the highlight by `delta` rows and stops at the ends.
    /// Moving down from the last row scrolls to the very bottom of the body,
    /// where a project's closing "landed, dropped" line may wait.
    pub fn move_by(&mut self, delta: isize) {
        let last = self.rows().count().saturating_sub(1);
        if delta > 0 && self.selected >= last {
            self.scroll = usize::MAX;
        } else if delta < 0 && self.scroll == usize::MAX {
            self.scroll = self.shown;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }

    /// Moves the highlight one page, of `capacity` lines, up or down.
    pub fn page(&mut self, capacity: usize, down: bool) {
        let step = isize::try_from(capacity.saturating_sub(1).max(1)).unwrap_or(1);
        self.move_by(if down { step } else { -step });
    }

    /// Highlights the first row.
    pub fn home(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    /// Highlights the last row.
    pub fn end(&mut self) {
        self.selected = self.rows().count().saturating_sub(1);
        self.scroll = usize::MAX;
    }
}
