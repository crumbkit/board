//! The two children a signature needs: the signer, which shows the owner a
//! statement and signs it with Touch ID, and `ssh-keygen`, which checks a
//! signature against an allowed signers file. Approvals, owner's writes and
//! attention orders all go through them
//! (docs/architecture/writes/signing.md).

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use board_model::signature_namespace;

use crate::process::{Finished, Limits, Scope, Stopped, run};

/// How long the owner has to read an approval or an owner's write and
/// touch the sensor. With the 3 s verify and the 5 s lock wait it fits the
/// effects mod's 30 s limit on `board answer`.
const APPROVE_TIME: Duration = Duration::from_secs(20);

/// How long the owner has to read an order, whose context may be 64 KiB,
/// and touch the sensor.
const ORDER_TIME: Duration = Duration::from_secs(600);

/// How long `ssh-keygen` has to check a signature, which takes
/// milliseconds.
const VERIFY_TIME: Duration = Duration::from_secs(3);

/// The exit status with which attention-sign says the owner asked for
/// changes instead of signing.
const DECLINED: i32 = 3;

/// The line board-sign prints last on stderr, before it exits 1, when the
/// owner cancels in its window. Every other failure also exits 1, and
/// `AppKit` may write its own lines to stderr before this one.
pub const CANCELLED: &str = "board-sign: the owner cancelled, so nothing was signed";

/// The same line from attention-sign, the signer before board-sign, which
/// Board still runs when board-sign is not installed.
const ATTENTION_CANCELLED: &str = "attention-sign: the owner cancelled, so nothing was signed";

/// The principal a statement is signed as, in the allowed signers file.
pub const PRINCIPAL: &str = "owner";

/// The command the signer is given, which says what it signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    /// `approve`: an approval or an owner's write. attention-sign signs it
    /// in the namespace its first line names, and refuses an order.
    Approve,
    /// `sign`: an attention order. attention-sign signs it in the order's
    /// namespace, and refuses an approval or an owner's write.
    Sign,
}

impl Verb {
    const fn word(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Sign => "sign",
        }
    }

    /// How long the signer may run: 20 seconds for an approval, 10 minutes
    /// for an order.
    pub const fn time(self) -> Duration {
        match self {
            Self::Approve => APPROVE_TIME,
            Self::Sign => ORDER_TIME,
        }
    }
}

/// `ssh-keygen` reads a signature only from a file, and the statement comes
/// on stdin, so the signature reaches it on descriptor 3 through a here
/// document. It never sees a shell metacharacter: the shell expands `$3`
/// once and does not read the result again.
const VERIFY: &str =
    "exec /usr/bin/ssh-keygen -Y verify -f \"$1\" -I \"$2\" -n \"$3\" -s /dev/fd/3 3<<EOF\n$4\nEOF";

/// Why no good signature was made or found.
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("{} could not be run: {source}", program.display())]
    NotStarted {
        program: OsString,
        source: std::io::Error,
    },
    #[error("{} did not finish within {}s", program.display(), time.as_secs())]
    Timeout { program: OsString, time: Duration },
    #[error("the owner did not sign and asked for changes: {feedback}")]
    Declined { feedback: String },
    #[error("the owner cancelled, so {} signed nothing", program.display())]
    Cancelled { program: OsString },
    #[error("{} exited with {status}: {stderr}", program.display())]
    Failed {
        program: OsString,
        status: String,
        stderr: String,
    },
    #[error("{} printed something other than an SSH signature", program.display())]
    NotASignature { program: OsString },
    #[error("no allowed signers file is at {}", file.display())]
    NoSigners { file: std::path::PathBuf },
    #[error("{} is not trusted: {why}", file.display())]
    Untrusted {
        file: std::path::PathBuf,
        why: &'static str,
    },
    #[error("the signature does not verify for {PRINCIPAL} in {}: {stderr}", file.display())]
    Bad {
        file: std::path::PathBuf,
        stderr: String,
    },
}

/// Runs `program <verb> <statement>`, the signer's command line, and
/// returns the armored signature it prints, within the verb's time.
/// `environment` is all the signer sees. The program must be a regular file
/// that this user or root owns and that neither its group nor others may
/// write.
pub fn sign(
    program: &OsString,
    statement: &Path,
    environment: &[(&str, OsString)],
    verb: Verb,
) -> Result<String, SignError> {
    trusted(Path::new(program))?;
    let arguments = [verb.word().to_owned(), statement.display().to_string()];
    let limits = Limits {
        time: verb.time(),
        ..Limits::default()
    };
    let (_, finished) = run(
        Path::new(program),
        &arguments,
        Path::new("/"),
        environment,
        Vec::new(),
        limits,
        Scope::Group,
    );
    let finished = finished_or(program, verb.time(), finished)?;
    let stdout = String::from_utf8_lossy(&finished.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&finished.stderr);
    match finished.code {
        Some(0) if stdout.starts_with("-----BEGIN SSH SIGNATURE-----") => Ok(stdout),
        Some(0) => Err(SignError::NotASignature {
            program: program.clone(),
        }),
        Some(DECLINED) => Err(SignError::Declined {
            feedback: stdout.trim().to_owned(),
        }),
        Some(1)
            if last_line(&stderr)
                .is_some_and(|line| [CANCELLED, ATTENTION_CANCELLED].contains(&line)) =>
        {
            Err(SignError::Cancelled {
                program: program.clone(),
            })
        }
        _ => Err(failed(program, &finished)),
    }
}

/// Checks that `signature` is the owner's signature of `statement`, by a
/// key the allowed signers file `allowed` lists for the principal `owner`,
/// in the namespace the statement's header names.
/// The file must be trusted as the signer is.
pub fn verify(allowed: &Path, statement: &str, signature: &str) -> Result<(), SignError> {
    if !allowed.is_file() {
        return Err(SignError::NoSigners {
            file: allowed.to_owned(),
        });
    }
    trusted(allowed)?;
    let program = OsString::from("/bin/sh");
    let arguments = [
        "-c".to_owned(),
        VERIFY.to_owned(),
        "sh".to_owned(),
        allowed.display().to_string(),
        PRINCIPAL.to_owned(),
        signature_namespace(statement).to_owned(),
        signature.trim().to_owned(),
    ];
    let limits = Limits {
        time: VERIFY_TIME,
        ..Limits::default()
    };
    let (_, finished) = run(
        Path::new(&program),
        &arguments,
        Path::new("/"),
        &[],
        statement.as_bytes().to_vec(),
        limits,
        Scope::Group,
    );
    let finished = finished_or(&program, VERIFY_TIME, finished)?;
    if finished.code == Some(0) {
        return Ok(());
    }
    Err(SignError::Bad {
        file: allowed.to_owned(),
        stderr: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
    })
}

/// Refuses `file` unless it is a regular file, after links, that this user
/// or root owns and that neither its group nor others may write, so another
/// account cannot change the signer or the keys an approval trusts. A
/// program that runs as this user can still change them, which only the
/// sandbox an agent runs in stops.
fn trusted(file: &Path) -> Result<(), SignError> {
    use std::os::unix::fs::MetadataExt as _;
    let untrusted = |why| SignError::Untrusted {
        file: file.to_owned(),
        why,
    };
    let metadata = file
        .metadata()
        .map_err(|_| untrusted("it cannot be read"))?;
    if !metadata.is_file() {
        return Err(untrusted("it is not a regular file"));
    }
    let owner = metadata.uid();
    if owner != rustix::process::getuid().as_raw() && owner != 0 {
        return Err(untrusted("another user owns it"));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(untrusted("its group or others may write it"));
    }
    Ok(())
}

/// The last line of `text` that holds more than whitespace, trimmed.
fn last_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).rfind(|line| !line.is_empty())
}

fn finished_or(
    program: &OsString,
    time: Duration,
    finished: Result<Finished, Stopped>,
) -> Result<Finished, SignError> {
    finished.map_err(|stopped| match stopped {
        Stopped::Io(source) => SignError::NotStarted {
            program: program.clone(),
            source,
        },
        Stopped::Timeout(_) => SignError::Timeout {
            program: program.clone(),
            time,
        },
        Stopped::TooLarge(_) => SignError::NotASignature {
            program: program.clone(),
        },
    })
}

fn failed(program: &OsString, finished: &Finished) -> SignError {
    SignError::Failed {
        program: program.clone(),
        status: finished
            .code
            .map_or_else(|| "a signal".to_owned(), |code| code.to_string()),
        stderr: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
    }
}

/// Board's allowed signers file with each key of the attention file merged
/// in for orders, as `board orders signers` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signers {
    /// Board's allowed signers file, which the owner writes the text to.
    pub file: std::path::PathBuf,
    /// The merged file, for the owner to read and write themselves.
    pub text: String,
    /// Each printed line that differs from Board's file.
    pub changes: Vec<Changed>,
    /// Each key Board's file lists on more than one line.
    pub duplicates: Vec<Duplicate>,
}

/// A printed line, numbered from 1, and how it differs from Board's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    pub line: usize,
    pub change: LineChange,
}

/// How a printed line differs from Board's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineChange {
    /// Board's line for a key the attention file lists, with the order
    /// namespace added to its `namespaces=`.
    Gained,
    /// A key only the attention file lists, admitted for orders alone.
    Added,
    /// Board's lines for one key, printed as the first of them with every
    /// line's principals and namespaces.
    Merged,
}

/// A key, as `<type> <base64>`, and the lines of Board's file, numbered
/// from 1, that list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    pub key: String,
    pub lines: Vec<usize>,
}

/// Board's allowed signers file `board` with each key of the attention file
/// `attention` merged in for `namespace`, read as `verify` reads them and
/// never written. A missing file reads as empty.
pub fn signers(
    board: &Path,
    attention: Option<&Path>,
    namespace: &str,
) -> Result<Signers, SignError> {
    let attention = attention.map(read_signers).transpose()?.unwrap_or_default();
    let mut merged = merged_signers(&read_signers(board)?, &attention, namespace);
    board.clone_into(&mut merged.file);
    Ok(merged)
}

/// The text of the allowed signers file `file`, empty when there is none.
fn read_signers(file: &Path) -> Result<String, SignError> {
    if !file.exists() {
        return Ok(String::new());
    }
    trusted(file)?;
    std::fs::read_to_string(file).map_err(|_| SignError::Untrusted {
        file: file.to_owned(),
        why: "it cannot be read as text",
    })
}

/// One key's line of an allowed signers file.
#[derive(Debug, Clone)]
struct Entry {
    principals: Vec<String>,
    options: Vec<String>,
    key: String,
    comment: Option<String>,
}

impl Entry {
    /// The line's key, its principals, options and comment around it, or
    /// none for a comment, a blank line or a line it cannot read.
    fn parse(line: &str) -> Option<Self> {
        let words = split_outside_quotes(line.trim(), char::is_whitespace);
        let is_type = |word: &str| {
            ["ssh-", "ecdsa-", "sk-"]
                .iter()
                .any(|t| word.starts_with(t))
        };
        let (options, at) = match words.as_slice() {
            [first, ..] if first.starts_with('#') => return None,
            [_, kind, _, ..] if is_type(kind) => (Vec::new(), 1),
            [_, options, kind, _, ..] if is_type(kind) => {
                (split_outside_quotes(options, |c| c == ','), 2)
            }
            _ => return None,
        };
        let comment = words[at + 2..].join(" ");
        Some(Self {
            principals: words[0].split(',').map(str::to_owned).collect(),
            options,
            key: format!("{} {}", words[at], words[at + 1]),
            comment: (!comment.is_empty()).then_some(comment),
        })
    }

    /// The namespaces its `namespaces=` names, or none when it has none and
    /// so admits every namespace.
    fn namespaces(&self) -> Option<Vec<String>> {
        let value = (self.options.iter()).find_map(|option| {
            let (name, value) = option.split_once('=')?;
            name.eq_ignore_ascii_case("namespaces").then_some(value)
        })?;
        Some(
            value
                .trim_matches('"')
                .split(',')
                .map(str::to_owned)
                .collect(),
        )
    }

    /// Sets its `namespaces=` to `namespaces`, or removes it for none.
    fn set_namespaces(&mut self, namespaces: Option<&[String]>) {
        let named = |option: &String| {
            (option.split_once('='))
                .is_some_and(|(name, _)| name.eq_ignore_ascii_case("namespaces"))
        };
        let at = self.options.iter().position(named);
        let option = namespaces.map(|all| format!("namespaces=\"{}\"", all.join(",")));
        match (at, option) {
            (Some(at), Some(option)) => self.options[at] = option,
            (Some(at), None) => drop(self.options.remove(at)),
            (None, Some(option)) => self.options.insert(0, option),
            (None, None) => {}
        }
    }

    fn line(&self) -> String {
        let mut words = vec![self.principals.join(",")];
        if !self.options.is_empty() {
            words.push(self.options.join(","));
        }
        words.push(self.key.clone());
        words.extend(self.comment.clone());
        words.join(" ")
    }
}

/// `text` split at each character `at` admits outside double quotes,
/// without empty pieces.
fn split_outside_quotes(text: &str, at: impl Fn(char) -> bool) -> Vec<String> {
    let (mut pieces, mut piece, mut quoted) = (Vec::new(), String::new(), false);
    for c in text.chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted && at(c) {
            pieces.push(std::mem::take(&mut piece));
        } else {
            piece.push(c);
        }
    }
    pieces.push(piece);
    pieces.retain(|piece| !piece.is_empty());
    pieces
}

/// A line of the printed file: Board's line as it was, or a key's line with
/// how it changed.
enum Printed {
    Kept(String),
    Key(Entry, Option<LineChange>, String),
}

/// `board`, the text of Board's allowed signers file, with each key of
/// `attention` admitted in `namespace`: Board's line for a key in both
/// gains `namespace` when its `namespaces=` lacks it; a key only in
/// `attention` gets `owner namespaces="<namespace>" <key>`; two lines of
/// Board's with one key print as one. Comments and other lines print as
/// they were.
fn merged_signers(board: &str, attention: &str, namespace: &str) -> Signers {
    let mut printed: Vec<Printed> = Vec::new();
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut duplicates: Vec<Duplicate> = Vec::new();
    for (number, line) in (1..).zip(board.lines()) {
        let Some(entry) = Entry::parse(line) else {
            printed.push(Printed::Kept(line.to_owned()));
            continue;
        };
        let Some(&first) = at.get(&entry.key) else {
            at.insert(entry.key.clone(), printed.len());
            printed.push(Printed::Key(entry, None, line.to_owned()));
            continue;
        };
        if let Printed::Key(kept, change, _) = &mut printed[first] {
            let namespaces = kept
                .namespaces()
                .zip(entry.namespaces())
                .map(|(mut all, more)| {
                    all.extend(
                        more.into_iter()
                            .filter(|n| !all.contains(n))
                            .collect::<Vec<_>>(),
                    );
                    all
                });
            kept.set_namespaces(namespaces.as_deref());
            for principal in entry.principals {
                if !kept.principals.contains(&principal) {
                    kept.principals.push(principal);
                }
            }
            *change = Some(LineChange::Merged);
        }
        match duplicates.iter_mut().find(|found| found.key == entry.key) {
            Some(found) => found.lines.push(number),
            None => duplicates.push(Duplicate {
                key: entry.key.clone(),
                lines: vec![first_line(board, &entry.key), number],
            }),
        }
    }
    for entry in attention.lines().filter_map(Entry::parse) {
        if let Some(&first) = at.get(&entry.key) {
            if let Printed::Key(kept, change, _) = &mut printed[first]
                && let Some(mut namespaces) = kept.namespaces()
                && !namespaces.iter().any(|n| n == namespace)
            {
                namespaces.push(namespace.to_owned());
                kept.set_namespaces(Some(&namespaces));
                change.get_or_insert(LineChange::Gained);
            }
            continue;
        }
        let added = Entry {
            principals: vec![PRINCIPAL.to_owned()],
            options: vec![format!("namespaces=\"{namespace}\"")],
            ..entry
        };
        at.insert(added.key.clone(), printed.len());
        printed.push(Printed::Key(added, Some(LineChange::Added), String::new()));
    }
    let mut text = String::new();
    let mut changes = Vec::new();
    for (number, line) in (1..).zip(&printed) {
        match line {
            Printed::Kept(line) | Printed::Key(_, None, line) => text.push_str(line),
            Printed::Key(entry, Some(change), _) => {
                text.push_str(&entry.line());
                changes.push(Changed {
                    line: number,
                    change: *change,
                });
            }
        }
        text.push('\n');
    }
    Signers {
        file: std::path::PathBuf::new(),
        text,
        changes,
        duplicates,
    }
}

/// The number, from 1, of the first line of `board` that lists `key`.
fn first_line(board: &str, key: &str) -> usize {
    (1..)
        .zip(board.lines())
        .find(|(_, line)| Entry::parse(line).is_some_and(|entry| entry.key == key))
        .map_or(0, |(number, _)| number)
}

#[cfg(test)]
mod tests;
