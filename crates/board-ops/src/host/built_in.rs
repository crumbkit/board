//! The plugins built into a Board, and how a board takes them.

use std::fmt;

use board_model::Moved;

use super::Plugin;
use crate::Board;

/// The plugins built into a Board.
#[derive(Clone, Copy, Default)]
pub struct BuiltIn(pub &'static [&'static dyn Plugin]);

impl fmt::Debug for BuiltIn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.iter().map(|plugin| plugin.manifest().name))
            .finish()
    }
}

impl Board {
    /// Builds `plugins` into this board's calls, ahead of installed ones.
    /// Its resolutions follow the moved prefixes these plugins'
    /// [`Plugin::moved`] gives. An installed plugin declares none, since its
    /// manifest may not name `moved`.
    ///
    /// # Panics
    ///
    /// When a plugin's moved pair is not two logical paths `Moved::new`
    /// admits. The pairs are constants, so every test that builds the
    /// plugins in fails on such a bug before a binary ships it.
    pub fn build_in(&mut self, plugins: &'static [&'static dyn Plugin]) {
        self.built_in = BuiltIn(plugins);
        let moved = (plugins.iter().flat_map(|plugin| plugin.moved()))
            .map(|(from, to)| {
                let (from, to) = (from.parse().ok(), to.parse().ok());
                (from.zip(to).and_then(|(from, to)| Moved::new(from, to)))
                    .expect("a built-in plugin declares a valid moved prefix")
            })
            .collect();
        self.moving(moved);
    }
}
