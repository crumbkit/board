//! The statement the owner signs for an attention order
//! (docs/architecture/writes/signing.md).

use std::fmt;

/// The `$type` of an attention order, which is also the first line of its
/// statement and the namespace the statement is signed in.
pub const ORDER_TYPE: &str = "attention/order/v1";

/// The date an order expires, which the caller parsed from the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expiry {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl fmt::Display for Expiry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

/// Whether an order is shown to every agent session as it starts. An order
/// that does not say is not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pinned {
    Yes,
    #[default]
    No,
}

impl From<bool> for Pinned {
    fn from(pinned: bool) -> Self {
        if pinned { Self::Yes } else { Self::No }
    }
}

/// An attention order and the text of its context file, which together
/// make the statement the owner signs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    /// The binding instruction, as the record stores it.
    pub order: String,
    /// The context file's path, as the record names it.
    pub context_path: String,
    pub expires: Expiry,
    pub pinned: Pinned,
    /// The context file's text, as the board stores it.
    pub context_text: String,
}

impl Order {
    /// The statement the owner signs, byte for byte the one board-attention
    /// built: the type, the expiry, whether it is pinned, the order and its
    /// context, each labelled. The order and the context text are copied as
    /// stored, so the statement ends with the context file's last byte.
    ///
    /// Every stored signature of an order is checked against this text, so
    /// its layout is frozen: a change to it needs a new first line.
    pub fn statement(&self) -> String {
        let pinned = match self.pinned {
            Pinned::Yes => "yes",
            Pinned::No => "no",
        };
        format!(
            "{ORDER_TYPE}\nexpires: {}\npinned: {pinned}\n\nOrder (binding):\n{}\n\n\
             Context (non-binding, from {}):\n{}",
            self.expires, self.order, self.context_path, self.context_text
        )
    }
}

#[cfg(test)]
mod tests;
