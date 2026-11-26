//! Why `board put` refuses a record, and how its message says so.

use board_model::{Code, Fields, FrontmatterError, JsonError};

use crate::verdict::Fault;

/// Why a record is refused, with what failed when Board knows more than the
/// code says.
#[derive(Debug, thiserror::Error)]
pub enum Refused {
    #[error(transparent)]
    Json(#[from] JsonError),
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error(transparent)]
    Fault(#[from] Fault),
    #[error("it is empty")]
    Empty,
    #[error("it is not a json object")]
    NotAnObject,
    #[error("it has no $type")]
    NoType,
    /// A record still refused after a command-line put dropped the fields
    /// its type does not name, or could not fill one.
    #[error("{}", mended(refused, dropped, unfilled))]
    Mended {
        refused: Box<Self>,
        dropped: Vec<String>,
        unfilled: Vec<String>,
    },
}

impl Refused {
    pub fn code(&self) -> Code {
        match self {
            Self::Json(error) => error.code(),
            Self::Frontmatter(error) => error.code(),
            Self::Fault(fault) => fault.code(),
            Self::Mended { refused, .. } => refused.code(),
            Self::Empty => Code::JsonParse,
            Self::NotAnObject | Self::NoType => Code::PutInvalid,
        }
    }

    /// The type and schema file of a record with no usable schema, or the
    /// pointer a schema refused.
    pub fn fields(&self) -> Fields {
        match self {
            Self::Fault(fault) => fault.fields(),
            Self::Mended { refused, .. } => refused.fields(),
            Self::Json(_)
            | Self::Frontmatter(_)
            | Self::Empty
            | Self::NotAnObject
            | Self::NoType => Fields::default(),
        }
    }
}

/// What a refused record is, after "the record" or its path.
pub(super) fn said(refused: &Refused) -> String {
    match refused {
        Refused::Fault(Fault::Code(_)) => "is refused".to_owned(),
        Refused::Fault(Fault::Record(error)) => error.to_string(),
        Refused::Mended {
            refused,
            dropped,
            unfilled,
        } => said(refused) + &lost(dropped, unfilled),
        _ => format!("is refused: {refused}"),
    }
}

/// A mended refusal after "is refused: ".
fn mended(refused: &Refused, dropped: &[String], unfilled: &[String]) -> String {
    refused.to_string() + &lost(dropped, unfilled)
}

/// What a refused record lost before its check: the fields dropped, and
/// why each field that could not be filled was not.
fn lost(dropped: &[String], unfilled: &[String]) -> String {
    let dropped = (!dropped.is_empty()).then(|| {
        format!(
            "; before the check, board put dropped {}, which its type does not name",
            dropped.join(", ")
        )
    });
    let unfilled = unfilled
        .iter()
        .map(|why| format!("; board put could not fill {why}"));
    dropped.into_iter().chain(unfilled).collect()
}
