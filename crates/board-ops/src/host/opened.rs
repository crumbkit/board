//! How a host opens boards for a built-in plugin: the called board again
//! as the plugin, the personal board, and the board `board decide` reads.

use std::path::Path;

use board_model::{Code, Fields, LogicalPath, PluginName, Refusal};
use board_plugin::Target;
use serde_json::Value;

use super::Hosted;
use crate::effects::Asker;
use crate::known::{self, KnownBoards};
use crate::put::{PutError, PutReport};
use crate::{Board, OpenError};

impl Hosted<'_> {
    /// The board opened again, with the same built-in plugins and build,
    /// and the same mount read at a checkout, as the plugin's own `board`
    /// opens it. Its timings stay in the board it opens, outside the call's
    /// trace.
    pub(super) fn again(&self) -> Result<Board, OpenError> {
        let mut board = self.board.open_beside(self.board.repository().path())?;
        board.build_in(self.board.built_in.0);
        board.built_by(self.board.build);
        if let Some(checkout) = self.board.repositories().checkout() {
            board.read_checkout(checkout.clone());
        }
        Ok(board)
    }

    /// Opens the board again as the plugin, as a put through a mount opens
    /// the mounted board, so every check reads the plugin where it already
    /// does, and writes `lines` there, marking the commit with `bulk`.
    pub(super) fn put_marked(
        &self,
        lines: &[Value],
        bulk: Option<&crate::Bulk>,
    ) -> Result<PutReport, PutError> {
        let (board, text) = self.batch_of(lines, bulk)?;
        board.put_batch(&mut text.as_bytes())
    }

    /// The board opened as the plugin, and `lines` as the text of a batch.
    pub(super) fn batch_of(
        &self,
        lines: &[Value],
        bulk: Option<&crate::Bulk>,
    ) -> Result<(Board, String), PutError> {
        let plugin = self.plugin()?;
        let mut board = self
            .again()
            .map_err(|error| PutError::Open(Box::new(error)))?;
        board.ask_as(Asker {
            plugin: Some(plugin),
            ..Asker::default()
        });
        if let Some(bulk) = bulk {
            board.mark_bulk(bulk.clone());
        }
        let text = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        Ok((board, text))
    }

    pub(super) fn plugin(&self) -> Result<PluginName, PutError> {
        (self.plugin.clone()).map_err(|text| PutError::BadPlugin { text })
    }
}

/// The logical path of each of `places`, or the refusal of the first whose
/// path is too long to be one, so a caller never mistakes a dropped place
/// for no place.
pub(super) fn prefixes(places: &[board_tree::Place]) -> Result<Vec<LogicalPath>, Box<Refusal>> {
    (places.iter())
        .map(|place| {
            (place.prefix())
                .map_err(|error| Box::new(Refusal::new(error.code(), &error, &Fields::default())))
        })
        .collect()
}

pub(super) fn opened(error: &OpenError) -> Refusal {
    Refusal::new(error.code(), error, &error.fields())
}

impl Hosted<'_> {
    /// The personal board the known boards file names, opened, or none when
    /// it names none or it does not open.
    pub(super) fn personal(&self) -> Option<Board> {
        let home = self.board.repositories().home();
        let root = self.plugins.known?.personal(home).ok()?;
        Board::open(&root, home.map(Path::to_path_buf)).ok()
    }

    /// Opens the board again asking as the plugin, with the personal
    /// board and signing `board decide` reads.
    pub(super) fn asking(&self, capability: &str) -> Result<Board, Box<Refusal>> {
        if !matches!(capability.parse(), Ok(Target::Named { .. })) {
            let message = format!(
                "the capability {capability:?} to decide must be `<plugin>.<capability>`, such as review.comment"
            );
            let refusal = Refusal::new(Code::CliUsage, message, &Fields::default());
            return Err(Box::new(refusal.with_hint(
                "name the capability as the plugin's name, a dot and the capability, as `board decide --capability` takes it",
            )));
        }
        let plugin = self.plugin().map_err(|error| Box::new(error.refusal()))?;
        let default = KnownBoards::default();
        let known = self.plugins.known.unwrap_or(&default);
        let home = self.board.repositories().home().map(Path::to_path_buf);
        let asker = known::asker(
            Some(plugin),
            Some(known),
            home,
            self.plugins.signing.clone(),
        )?;
        let mut board = self.again().map_err(|error| Box::new(opened(&error)))?;
        board.ask_as(asker);
        Ok(board)
    }
}
