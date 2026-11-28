//! The lines `board get` prints on stderr beside a record, from the one
//! file a built-in plugin reads beside it, such as the answer beside a
//! question (docs/architecture/records/get.md, Notes beside a record).

use board_model::{Code, LogicalPath};
use serde_json::{Map, Value};

use crate::board::Board;
use crate::history::At;
use crate::read::{GetReport, ReadError};

/// A line `board get` prints on stderr beside a record of one type. A
/// built-in plugin declares its notes with [`crate::Plugin::get_notes`].
#[derive(Debug)]
pub struct GetNote {
    /// The record type, such as `plans/question/v1`.
    pub record_type: &'static str,
    /// The one path to read beside such a record, or `None` to read
    /// nothing and print nothing.
    pub beside: fn(&Asked) -> Option<LogicalPath>,
    /// The line from the record and what is at the path beside it, or
    /// `None` for no line.
    pub line: fn(&Asked, &Beside<'_>) -> Option<String>,
}

/// A record `board get` read, of a type a [`GetNote`] names.
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    /// Its path as the reader asked it.
    pub path: LogicalPath,
    /// Its logical path once every pointer is followed.
    pub display: LogicalPath,
    /// Its path in the board or repository that holds the file, such as
    /// `/projects/p/questions/q` for a question read through a mount,
    /// without `.json`.
    pub held: String,
    /// The record's fields.
    pub record: Map<String, Value>,
}

/// What `board get` found at the path beside a record.
#[derive(Debug)]
pub enum Beside<'a> {
    /// A record, of any type.
    Record {
        path: &'a LogicalPath,
        record: &'a Value,
    },
    /// Nothing at the path, or a file there that is not a record.
    Nothing { path: &'a LogicalPath },
    /// The read failed other than by finding nothing.
    Failed { path: &'a LogicalPath, code: Code },
}

impl Board {
    /// Every [`GetNote`] the built-in plugins declare.
    pub fn get_notes(&self) -> Vec<&'static GetNote> {
        (self.built_in.0.iter())
            .flat_map(|plugin| plugin.get_notes())
            .collect()
    }

    /// `report`, read at `path`, as a record a built-in [`GetNote`] names;
    /// `None` when it is not a record or no note names its type.
    pub fn asked(&self, path: &LogicalPath, report: &GetReport) -> Option<Asked> {
        let Ok(Value::Object(record)) = &report.record else {
            return None;
        };
        let record_type = record.get("$type").and_then(Value::as_str)?;
        (self.get_notes().iter())
            .any(|note| note.record_type == record_type)
            .then(|| Asked {
                path: path.clone(),
                display: report.display.clone(),
                held: held(&report.location.path),
                record: record.clone(),
            })
    }

    /// The lines beside each of `asked`, in order, reading every path
    /// beside them in one read, in the commit `at` names or in the working
    /// tree.
    pub fn noted(&self, at: Option<At>, asked: &[Asked]) -> Vec<String> {
        lines(&self.get_notes(), asked, |paths| {
            let each: Vec<_> = match at {
                None => self.get_each(paths)?.collect(),
                Some(at) => self.get_each_at(at, paths)?.collect(),
            };
            Ok(each)
        })
    }
}

/// `path`, inside the tree that holds a file, as a logical path: with a
/// leading `/` and without `.json`.
fn held(path: &str) -> String {
    format!("/{}", path.strip_suffix(".json").unwrap_or(path))
}

/// One read's answers: one for each path, in order, or the one error that
/// stopped the read.
type Read = Result<Vec<Result<GetReport, ReadError>>, ReadError>;

/// The lines `notes` give beside each of `asked`, reading the paths beside
/// them with one call of `read`, which is not called when there are none.
pub(crate) fn lines(
    notes: &[&'static GetNote],
    asked: &[Asked],
    read: impl FnOnce(&[LogicalPath]) -> Read,
) -> Vec<String> {
    let pairs: Vec<(&Asked, &GetNote, LogicalPath)> = (asked.iter())
        .flat_map(|asked| {
            let record_type = asked.record.get("$type").and_then(Value::as_str);
            (notes.iter())
                .filter(move |note| Some(note.record_type) == record_type)
                .filter_map(move |note| Some((asked, *note, (note.beside)(asked)?)))
        })
        .collect();
    if pairs.is_empty() {
        return Vec::new();
    }
    let paths: Vec<LogicalPath> = pairs.iter().map(|(.., path)| path.clone()).collect();
    let read = read(&paths);
    (pairs.iter().enumerate())
        .filter_map(|(index, (asked, note, path))| {
            let beside = match &read {
                Err(error) => Beside::Failed {
                    path,
                    code: error.code(),
                },
                Ok(each) => match each.get(index) {
                    Some(Ok(report)) => match &report.record {
                        Ok(record) => Beside::Record { path, record },
                        Err(_) => Beside::Nothing { path },
                    },
                    Some(Err(error)) if error.code() == Code::PathNotFound => {
                        Beside::Nothing { path }
                    }
                    Some(Err(error)) => Beside::Failed {
                        path,
                        code: error.code(),
                    },
                    None => Beside::Nothing { path },
                },
            };
            (note.line)(asked, &beside)
        })
        .collect()
}

#[cfg(test)]
mod tests;
