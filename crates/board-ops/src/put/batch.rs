//! `board put --batch`: every line checked and every expect compared before
//! one commit that writes and removes, in this board or through one mount (docs/architecture.md,
//! Writes; docs/architecture/writes.md).

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read};
use std::time::Duration;

use board_model::{
    BINARY_NAME, Code, Fields, Lined, LogicalPath, MAX_RECORD_BYTES, ObjectId, RecordType,
    parse_json,
};
use board_tree::{LOCK_WAIT, Lock, Resolver, WriteError};
use serde_json::Value;

use super::new::{Draw, REDRAWS, drawn, folder_text, redrawing, taken, under};
use super::through::{Crossing, Sent};
use super::{Expect, Failure, Prepared, PutError, PutReport, Removal, Schemas, Unmatched};
use crate::board::Board;
use crate::effects::{Change, Shape};
use crate::types::TypeError;

/// The most lines one batch may hold.
pub const MAX_BATCH_LINES: usize = 10_000;

/// One line of `put --batch`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    new: Option<String>,
    #[serde(default)]
    expect: Option<String>,
    #[serde(default)]
    record: Option<Value>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    remove: bool,
    #[serde(default)]
    ignore_unmatch: bool,
}

/// A batch line read with its number, or why it is refused.
pub(crate) type ReadLine = Result<(usize, Line), Lined>;

/// One batch line, read but not yet checked: `shown` is its path as the
/// caller gave it, and `path` where it is checked. `new` is the folder of a
/// line that `new` names.
#[derive(Clone)]
pub(crate) struct Line {
    path: LogicalPath,
    shown: LogicalPath,
    expect: Expect,
    content: Content,
    new: Option<LogicalPath>,
}

/// What a batch line writes: a record, or a file's bytes as given; or
/// that it removes the file.
#[derive(Clone)]
enum Content {
    Record(Vec<u8>),
    File(Vec<u8>),
    Remove(Unmatched),
}

/// What a checked line does: writes, removes, or finds its path absent.
enum Step {
    Write(Box<Prepared>),
    Remove(Removal),
    Absent(LogicalPath),
}

impl Step {
    /// The file in the working tree the step changes, if any.
    fn file(&self) -> Option<&str> {
        match self {
            Self::Write(prepared) => Some(&prepared.target.path),
            Self::Remove(removal) => Some(&removal.file),
            Self::Absent(_) => None,
        }
    }
}

/// A batch whose every line was checked: prepared, removed and found
/// absent in this board, with each line's number, the writes' first, or
/// leading through one mount, with the lines as the mounted board reads
/// them.
pub(crate) enum Checked {
    Here(Vec<Prepared>, Vec<Removal>, Vec<LogicalPath>, Vec<usize>),
    Through(Vec<Crossing>, Vec<ReadLine>),
}

impl Line {
    /// The type whose schema the line writes, when it is a record line at
    /// a schema's path.
    fn schema_type(&self) -> Option<RecordType> {
        let file = format!("{}.json", self.path.as_str().trim_start_matches('/'));
        match self.content {
            Content::Record(_) => RecordType::of_schema(&file),
            Content::File(_) | Content::Remove(_) => None,
        }
    }
}

impl Board {
    /// Reads JSON Lines of `{path, expect, record}`, `{path, expect, text}`,
    /// `{path, expect, remove: true}` or `{new, record}` from `input`, then
    /// takes the lock, waiting up to [`LOCK_WAIT`], checks every line and
    /// compares every expect, and writes and removes them only if none
    /// refuses, with one commit.
    pub fn put_batch(&self, input: &mut dyn BufRead) -> Result<PutReport, PutError> {
        self.put_batch_waiting(input, LOCK_WAIT)
    }

    /// [`Board::put_batch`], waiting up to `wait` for the lock, and for a
    /// mounted board's lock.
    pub fn put_batch_waiting(
        &self,
        input: &mut dyn BufRead,
        wait: Duration,
    ) -> Result<PutReport, PutError> {
        self.put_batch_named(input, wait, &mut drawn())
    }

    /// [`Board::put_batch_waiting`], naming each `new` line with the names
    /// `draw` gives, and drawing again while only those names are taken.
    pub(crate) fn put_batch_named(
        &self,
        input: &mut dyn BufRead,
        wait: Duration,
        draw: Draw<'_>,
    ) -> Result<PutReport, PutError> {
        let lines = Self::read_lines(input)?;
        if lines.is_empty() {
            return Ok(PutReport::new(Vec::new(), None));
        }
        if lines.iter().all(Result::is_err) {
            return Err(PutError::Batch {
                code: Code::PutInvalid,
                refusals: lines.into_iter().filter_map(Result::err).collect(),
            });
        }
        let new: HashMap<usize, LogicalPath> = (lines.iter().flatten())
            .filter_map(|(number, line)| Some((*number, line.new.clone()?)))
            .collect();
        if new.is_empty() {
            return self.write_lines(lines, wait);
        }
        let taken_only = |error: &PutError| match error {
            PutError::Batch { code, refusals } => {
                *code == Code::PutStale && refusals.iter().all(|line| new.contains_key(&line.line))
            }
            _ => false,
        };
        let put = || self.write_lines(named(&lines, &mut *draw)?, wait);
        // A new line found stale after the redraws is a taken name,
        // whatever the other lines did.
        match redrawing(put, taken_only) {
            Err(PutError::Batch { mut refusals, .. }) => {
                for refused in &mut refusals {
                    if let (Code::PutStale, Some(folder)) = (refused.code, new.get(&refused.line)) {
                        let error = taken(folder);
                        *refused = Lined::new(refused.line, error.code(), &error, error.fields());
                    }
                }
                let stale = refusals
                    .iter()
                    .all(|refused| refused.code == Code::PutStale);
                let code = if stale {
                    Code::PutStale
                } else {
                    Code::PutInvalid
                };
                Err(PutError::Batch { code, refusals })
            }
            put => put,
        }
    }

    /// Writes `lines` as [`Board::put_batch_waiting`] says.
    fn write_lines(&self, lines: Vec<ReadLine>, wait: Duration) -> Result<PutReport, PutError> {
        let lock = self.repository().lock(wait)?;
        let mut view = self.repositories().snapshot().map_err(WriteError::from)?;
        let (prepared, removals, absent, numbers) =
            match self.check_lines(&lock, &mut view, lines, &mut Schemas::default())? {
                Checked::Here(prepared, removals, absent, numbers) => {
                    (prepared, removals, absent, numbers)
                }
                Checked::Through(crossings, lines) => {
                    return self.put_through(lock, &crossings, Sent::Batch(lines), wait);
                }
            };
        let message = message(&prepared, removals.len());
        let report = self.write_all(lock, &prepared, &removals, &message);
        report
            .map(|report| PutReport { absent, ..report })
            .map_err(|failure| match failure {
                Failure::Stale(stale) => refused_lines(
                    &stale
                        .into_iter()
                        .map(|(at, error)| (numbers[at], error))
                        .collect::<Vec<_>>(),
                ),
                Failure::Error(error) => error,
            })
    }

    /// Reads every line of a batch, each with its number or why it is
    /// refused.
    pub(crate) fn read_lines(input: &mut dyn BufRead) -> Result<Vec<ReadLine>, PutError> {
        let mut lines = Vec::new();
        let mut line = Vec::new();
        for number in 1.. {
            line.clear();
            let read = input
                .take(MAX_RECORD_BYTES as u64 + 2)
                .read_until(b'\n', &mut line)
                .map_err(|source| PutError::Input { source })?;
            if read == 0 {
                break;
            }
            if number > MAX_BATCH_LINES {
                return Err(PutError::TooManyLines);
            }
            let text = line.strip_suffix(b"\n").unwrap_or(&line);
            if text.len() > MAX_RECORD_BYTES {
                // The rest of the line has not been read, so no later line
                // can be found.
                lines.push(Err(Lined::new(
                    number,
                    Code::JsonTooLarge,
                    format!("the line is over {MAX_RECORD_BYTES} bytes"),
                    Fields::default(),
                )));
                break;
            }
            if text.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            lines.push(match Self::parse_line(text) {
                Ok(parsed) => Ok((number, parsed)),
                Err(error) => Err(Lined::new(number, error.code(), &error, error.fields())),
            });
        }
        Ok(lines)
    }

    /// Checks every line of a batch under `lock` against the one snapshot
    /// `view`, schema lines first so that a record can use a schema the
    /// batch adds, and returns what it checked, or every refusal.
    pub(crate) fn check_lines<'b>(
        &'b self,
        lock: &Lock,
        view: &mut Resolver<'_, '_>,
        lines: Vec<ReadLine>,
        schemas: &mut Schemas<'b>,
    ) -> Result<Checked, PutError> {
        let mut steps: Vec<(usize, Step)> = Vec::new();
        let mut refusals: Vec<Lined> = Vec::new();
        let (mut crossings, mut through) = (Vec::new(), Vec::new());
        // Each line checked and the mount it writes through, if any.
        let mut boards: Vec<(usize, Option<LogicalPath>)> = Vec::new();
        // Each target folded as the walk folds names, with the line and the
        // path that wrote it, so two lines that a case-insensitive file
        // system would make one file clash.
        let mut folded: HashMap<String, (usize, String, LogicalPath)> = HashMap::new();
        let (schema_lines, lines): (Vec<_>, Vec<_>) = lines
            .into_iter()
            .partition(|line| matches!(line, Ok((_, line)) if line.schema_type().is_some()));
        for parsed in schema_lines.into_iter().chain(lines) {
            let (number, line) = match parsed {
                Ok(parsed) => parsed,
                Err(refused) => {
                    refusals.push(refused);
                    continue;
                }
            };
            let shown = line.shown.clone();
            let (item, board) = match self.check_line(lock, view, schemas, &line) {
                Ok(item) => (item, None),
                // `inside` is only set for one mount.
                Err(PutError::Write(WriteError::ThroughMount {
                    mount,
                    inside: Some(inside),
                    ..
                })) => {
                    let path = inside.path.clone();
                    crossings.push(Crossing {
                        number,
                        path: line.path,
                        mount: mount.clone(),
                        inside: *inside,
                    });
                    through.push(Ok((number, Line { path, ..line })));
                    (None, Some(mount))
                }
                Err(error) => {
                    refusals.push(Lined::new(number, error.code(), &error, error.fields()));
                    continue;
                }
            };
            boards.push((number, board));
            let Some(item) = item else { continue };
            let Some(written) = item.file().map(str::to_owned) else {
                steps.push((number, item));
                continue;
            };
            match folded.entry(written.to_ascii_lowercase()) {
                Entry::Occupied(first) => {
                    let (first, file, earlier) = first.get();
                    let (code, message) = if *file == written {
                        (Code::PutInvalid, "the same file as")
                    } else {
                        (Code::PathCaseCollision, "differing only in case from")
                    };
                    let message = format!("it writes {shown}, {message} line {first}'s {earlier}");
                    refusals.push(Lined::new(number, code, message, Fields::at(&shown)));
                }
                Entry::Vacant(entry) => {
                    entry.insert((number, written, shown));
                }
            }
            steps.push((number, item));
        }
        refusals.extend(spanning(boards));
        if !refusals.is_empty() {
            refusals.sort_by_key(|refused| refused.line);
            return Err(PutError::Batch {
                code: Code::PutInvalid,
                refusals,
            });
        }
        // Schema lines were checked first; writes, removals and the report
        // go in input order.
        steps.sort_by_key(|(number, _)| *number);
        let (mut prepared, mut removals, mut absent) = (Vec::new(), Vec::new(), Vec::new());
        let (mut numbers, mut removed) = (Vec::new(), Vec::new());
        for (number, step) in steps {
            match step {
                Step::Write(item) => (prepared.push(*item), numbers.push(number)),
                Step::Remove(removal) => (removals.push(removal), removed.push(number)),
                Step::Absent(path) => (absent.push(path), ()),
            };
        }
        numbers.extend(removed);
        Ok(if crossings.is_empty() {
            Checked::Here(prepared, removals, absent, numbers)
        } else {
            Checked::Through(crossings, through)
        })
    }

    /// Checks one line: a record, a schema, a file or a removal. A schema
    /// the board holds through a symlink, and equal, is not written.
    fn check_line<'b>(
        &'b self,
        lock: &Lock,
        view: &mut Resolver<'_, '_>,
        schemas: &mut Schemas<'b>,
        line: &Line,
    ) -> Result<Option<Step>, PutError> {
        let Line {
            path,
            shown,
            expect,
            content,
            ..
        } = line;
        let bytes = match content {
            Content::File(bytes) => {
                return self
                    .prepare_file(lock, view, schemas, path, *expect, bytes)
                    .map(|item| Some(Step::Write(Box::new(item))))
                    .map_err(|error| shown_as(error, shown));
            }
            Content::Remove(unmatched) => {
                return match self.prepare_removal(lock, view, path, *expect) {
                    Err(error) if unmatched.ignores(&error) => Ok(Some(Step::Absent(path.clone()))),
                    removal => removal.map(|removal| Some(Step::Remove(removal))),
                };
            }
            Content::Record(bytes) => bytes,
        };
        let Some(record_type) = line.schema_type() else {
            return self
                .prepare(lock, view, schemas, path, *expect, bytes)
                .map(|item| Some(Step::Write(Box::new(item))))
                .map_err(|error| shown_as(error, shown));
        };
        if *expect != Expect::Any {
            return Err(PutError::Line {
                code: Code::PutInvalid,
                reason: format!(
                    "{shown} is a schema, which a batch writes when the board lacks it and leaves when it holds it, so its line takes no expect"
                ),
            });
        }
        let (prepared, compiled) = Self::prepare_type(
            lock,
            (view, Schemas::reader(&mut schemas.reader, self)),
            &record_type,
            bytes,
        )
        .map_err(|error| match error {
            TypeError::Put(error) => shown_as(error, shown),
            error => PutError::Type(Box::new(error)),
        })?;
        if let Some(prepared) = &prepared {
            let change = Change::Put(&prepared.canonical, &prepared.admitted, Shape::Schema);
            self.gate(&prepared.target, change)?;
        }
        schemas.held.insert(record_type, Ok(compiled));
        schemas.compiled += 1;
        // Through a mount, an expect is compared with the pin, which may hold
        // a schema the working tree lacks; that is a moved file, not a stale
        // expect.
        Ok(prepared.map(|prepared| {
            Step::Write(Box::new(Prepared {
                expect: Expect::Any,
                ..prepared
            }))
        }))
    }

    /// Reads one batch line.
    fn parse_line(line: &[u8]) -> Result<Line, PutError> {
        let invalid = |reason: String| PutError::Line {
            code: Code::PutInvalid,
            reason: format!(
                "the line is not {{path, expect, record}}, {{path, expect, text}}, {{path, expect, remove}} or {{new, record}}: {reason}"
            ),
        };
        let value = parse_json(line).map_err(|error| PutError::Line {
            code: error.code(),
            reason: error.to_string(),
        })?;
        let raw: RawLine =
            serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
        let (given, new) = match (&raw.path, &raw.new) {
            (Some(path), None) => (path, false),
            (None, Some(folder)) => (folder, true),
            (Some(_), Some(_)) => return Err(invalid("it holds both path and new".to_owned())),
            (None, None) => return Err(invalid("it holds no path or new".to_owned())),
        };
        let text = if new { folder_text(given) } else { given };
        let path: LogicalPath = text
            .parse()
            .map_err(|_| invalid(format!("{given:?} is not a logical path")))?;
        if new && !given.ends_with('/') {
            return Err(PutError::NotAFolder { folder: path });
        }
        if new && (raw.expect.is_some() || raw.record.is_none()) {
            return Err(invalid(
                "a new line holds a record and no expect, since its name is new".to_owned(),
            ));
        }
        let expect = match raw.expect.as_deref() {
            None => Expect::Any,
            Some("none") => Expect::Absent,
            Some(hex) => ObjectId::from_hex(hex)
                .map(Expect::Blob)
                .ok_or_else(|| invalid(format!("expect {hex:?} is not `none` or a blob id")))?,
        };
        if raw.ignore_unmatch && !raw.remove {
            return Err(invalid(
                "it ignores an unmatched path and does not remove".to_owned(),
            ));
        }
        let unmatched = if raw.ignore_unmatch {
            Unmatched::Ignore
        } else {
            Unmatched::Refuse
        };
        let content = match (raw.record, raw.text, raw.remove) {
            (Some(record), None, false) => Content::Record(
                serde_json::to_vec(&record).map_err(|error| invalid(error.to_string()))?,
            ),
            (None, Some(text), false) => Content::File(text.into_bytes()),
            (None, None, true) => Content::Remove(unmatched),
            (Some(_), Some(_), _) => {
                return Err(invalid("it holds both record and text".to_owned()));
            }
            (_, _, true) => {
                return Err(invalid(
                    "it removes and also holds a record or text".to_owned(),
                ));
            }
            (None, None, false) => {
                return Err(invalid(
                    "it holds no record or text and does not remove".to_owned(),
                ));
            }
        };
        Ok(Line {
            shown: path.clone(),
            new: new.then(|| path.clone()),
            path,
            expect: if new { Expect::Absent } else { expect },
            content,
        })
    }
}

/// `lines`, each `new` line with a path drawn under its folder, drawn
/// again up to [`REDRAWS`] times when an earlier line has it.
fn named(lines: &[ReadLine], draw: Draw<'_>) -> Result<Vec<ReadLine>, PutError> {
    let mut held = HashSet::new();
    let mut named = lines.to_vec();
    for (_, line) in named.iter_mut().flatten() {
        let Some(folder) = &line.new else { continue };
        line.path = under(folder, &mut *draw)?;
        for _ in 0..REDRAWS {
            if held.insert(line.path.clone()) {
                break;
            }
            line.path = under(folder, &mut *draw)?;
        }
        line.shown = line.path.clone();
    }
    Ok(named)
}

/// A refusal for each line in another board than the batch's lowest-numbered
/// line, each given with the mount it writes through, if any.
fn spanning(mut boards: Vec<(usize, Option<LogicalPath>)>) -> Vec<Lined> {
    boards.sort_by_key(|(number, _)| *number);
    let Some((first, there)) = boards.first().cloned() else {
        return Vec::new();
    };
    let name = |board: &Option<LogicalPath>| {
        board.as_ref().map_or_else(
            || "this board".to_owned(),
            |mount| format!("the board mounted at {mount}"),
        )
    };
    let elsewhere = boards.into_iter().filter(|(_, board)| *board != there);
    elsewhere
        .map(|(number, board)| {
            let message = format!(
                "it writes in {}, and line {first} writes in {}; a batch writes in one board",
                name(&board),
                name(&there)
            );
            let fields = board.as_ref().or(there.as_ref()).map(Fields::at);
            Lined::new(
                number,
                Code::PutThroughMount,
                message,
                fields.unwrap_or_default(),
            )
        })
        .collect()
}

/// `error` naming `shown`, the path the caller gave, in place of the path
/// it was checked at, which differs through a mount.
pub(super) fn shown_as(mut error: PutError, shown: &LogicalPath) -> PutError {
    if let PutError::Record { path, .. }
    | PutError::Write(
        WriteError::Unresolved { path, .. }
        | WriteError::ThroughMount { path, .. }
        | WriteError::NotARecord { path, .. }
        | WriteError::InGit { path, .. }
        | WriteError::NotAFile { path, .. }
        | WriteError::Degraded { path, .. }
        | WriteError::Beneath { path, .. }
        | WriteError::Symlink { path, .. },
    ) = &mut error
    {
        path.clone_from(shown);
    }
    error
}

/// The commit message of a batch that writes `prepared` and removes
/// `removals` files, counting its records, its files, whose bytes are
/// admitted as no value, and its removals.
pub(super) fn message(prepared: &[Prepared], removals: usize) -> String {
    let files = prepared
        .iter()
        .filter(|item| item.admitted.value.is_null())
        .count();
    let records = prepared.len() - files;
    let counted = [
        (records, "records"),
        (files, "files"),
        (removals, "removals"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, what)| format!("{count} {what}"))
    .collect::<Vec<_>>();
    let counted = if counted.is_empty() {
        "0 records".to_owned()
    } else {
        counted.join(", ")
    };
    format!("{BINARY_NAME} put --batch ({counted})")
}

/// A batch refused for `failures`, each with its line number: stale when
/// every one is.
pub(super) fn refused_lines(failures: &[(usize, PutError)]) -> PutError {
    let stale = failures
        .iter()
        .all(|(_, error)| matches!(error, PutError::Stale { .. }));
    PutError::Batch {
        code: if stale {
            Code::PutStale
        } else {
            Code::PutInvalid
        },
        refusals: failures
            .iter()
            .map(|(number, error)| Lined::new(*number, error.code(), error, error.fields()))
            .collect(),
    }
}
