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
/// in for orders, as `board orders signers` prints it. No printed line
/// admits more than the lines it came from admitted.
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
    /// Each line of the attention file that gave the printed file nothing.
    pub left_out: Vec<LeftOut>,
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
    /// Board's line for a key the attention file admits in orders, with the
    /// order namespace added to its `namespaces=`.
    Gained,
    /// A key only the attention file lists, with that line's principals and
    /// options, admitted for orders alone.
    Added,
    /// Board's lines for one key with the same principals and other
    /// options, printed as one line holding both lines' namespaces.
    Merged,
}

/// A key, as `<type> <base64>`, the lines of Board's file, numbered from 1,
/// that list it, and whether they print as one line. Lines whose
/// principals or other options differ print unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    pub key: String,
    pub lines: Vec<usize>,
    pub merged: bool,
}

/// A line of the attention file, numbered from 1, that gave the printed
/// file nothing, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    pub line: usize,
    /// The key, as `<type> <base64>`, or none when the line cannot be read.
    pub key: Option<String>,
    pub reason: LeftOutReason,
}

/// Why a line of the attention file gave the printed file nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftOutReason {
    /// Its principals do not name `owner`.
    NotOwner,
    /// Its `namespaces=` leaves out the order namespace.
    NotOrders,
    /// Board's lines for its key name other principals or other options,
    /// so adding the order namespace would admit more than it does.
    Differs,
    /// It or Board's line for its key names principals or namespaces with
    /// `!`, `*` or `?`, which ssh-keygen reads as patterns, or with an
    /// element of 1023 bytes or more, which ssh-keygen matches nothing
    /// against, so the lists are not compared.
    Pattern,
    /// It cannot be read as ssh-keygen reads it: it holds whitespace other
    /// than spaces and tabs or a control character, its principals hold a
    /// double quote, its options hold an empty piece or a backslash, or its
    /// `namespaces=` is not one quoted value.
    Unreadable,
    /// An earlier line of the attention file already gave its key.
    Duplicate,
    /// Board's lines for its key already admit `owner` in orders.
    AlreadyAdmitted,
}

/// Board's allowed signers file `board` with each key of the attention file
/// `attention` merged in for orders, read as `verify` reads them and never
/// written. A missing file reads as empty.
pub fn signers(board: &Path, attention: Option<&Path>) -> Result<Signers, SignError> {
    let attention = attention.map(read_signers).transpose()?.unwrap_or_default();
    let mut merged = merged_signers(&read_signers(board)?, &attention);
    board.clone_into(&mut merged.file);
    Ok(merged)
}

/// Whether an allowed signers file lists the owner, which decides whether
/// Board's signing is on (docs/architecture/writes/signing.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listed {
    /// No file is there.
    Missing,
    /// The file is there and no line names `owner` among its principals.
    NoOwner,
    /// A line names `owner` among its principals.
    Owner,
    /// Whether a file is there cannot be told: the path is a link to
    /// nothing, or a directory on the way cannot be searched.
    Unreadable,
}

/// Whether the allowed signers file `file` has a line whose principals
/// name `owner` exactly. A file Board does not trust is refused, as
/// `verify` refuses it, before it is read. A link to nothing, or a path
/// whose directories cannot be searched, is [`Listed::Unreadable`].
pub fn owner_listed(file: &Path) -> Result<Listed, SignError> {
    match file.try_exists() {
        Ok(true) => {}
        Ok(false) if file.symlink_metadata().is_ok() => return Ok(Listed::Unreadable),
        Ok(false) => return Ok(Listed::Missing),
        Err(_) => return Ok(Listed::Unreadable),
    }
    let text = read_signers(file)?;
    let owner = (text.lines().filter_map(Entry::parse)).any(|entry| {
        entry
            .principals
            .iter()
            .any(|principal| principal == PRINCIPAL)
    });
    Ok(if owner {
        Listed::Owner
    } else {
        Listed::NoOwner
    })
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
    /// Its options other than `namespaces=`, such as `cert-authority` or
    /// `valid-before=`, as written.
    others: Vec<String>,
    /// The namespaces its `namespaces=` names, or none when it has none and
    /// so admits every namespace.
    namespaces: Option<Vec<String>>,
    key: String,
    comment: Option<String>,
}

/// Whether `option` is a `namespaces=` option.
fn is_namespaces(option: &str) -> bool {
    (option.split_once('=')).is_some_and(|(name, _)| name.eq_ignore_ascii_case("namespaces"))
}

impl Entry {
    /// The line's key, its principals, options and comment around it, or
    /// none for a comment, a blank line or a line it cannot read.
    fn parse(line: &str) -> Option<Self> {
        let line = trim_blanks(line);
        if line
            .chars()
            .any(|c| !is_blank(c) && (c.is_whitespace() || c.is_control()))
        {
            return None;
        }
        let mut words = split_outside_quotes(line, is_blank);
        words.retain(|word| !word.is_empty());
        let is_type = |word: &str| {
            ["ssh-", "ecdsa-", "sk-"]
                .iter()
                .any(|t| word.starts_with(t))
        };
        let (options, at) = match words.as_slice() {
            [first, ..] if first.starts_with('#') => return None,
            [_, kind, _, ..] if is_type(kind) => (Vec::new(), 1),
            [_, options, kind, _, ..] if is_type(kind) && !options.contains('\\') => {
                let options = split_outside_quotes(options, |c| c == ',');
                if options.iter().any(String::is_empty) {
                    return None;
                }
                (options, 2)
            }
            _ => return None,
        };
        if words[0].contains('"') {
            return None;
        }
        let namespaces = match options
            .iter()
            .filter(|option| is_namespaces(option))
            .collect::<Vec<_>>()[..]
        {
            [] => None,
            [option] => {
                let (_, value) = option.split_once('=').expect("is_namespaces found an =");
                let value = value.strip_prefix('"')?.strip_suffix('"')?;
                if value.contains('"') {
                    return None;
                }
                Some(value.split(',').map(str::to_owned).collect())
            }
            _ => return None,
        };
        let comment = words[at + 2..].join(" ");
        Some(Self {
            principals: words[0].split(',').map(str::to_owned).collect(),
            others: options
                .into_iter()
                .filter(|option| !is_namespaces(option))
                .collect(),
            namespaces,
            key: format!("{} {}", words[at], words[at + 1]),
            comment: (!comment.is_empty()).then_some(comment),
        })
    }

    /// Whether it admits `principal` in `namespace`, by its principals and
    /// `namespaces=` alone.
    fn admits(&self, principal: &str, namespace: &str) -> bool {
        self.principals.iter().any(|p| p == principal)
            && (self.namespaces.as_ref()).is_none_or(|all| all.iter().any(|n| n == namespace))
    }

    /// Whether its principals or namespaces hold a `!`, `*` or `?`, which
    /// ssh-keygen matches as a pattern, or an element of
    /// [`LONGEST_ELEMENT`] bytes or more, which makes ssh-keygen match
    /// nothing against the whole list.
    fn has_pattern(&self) -> bool {
        (self.principals.iter())
            .chain(self.namespaces.iter().flatten())
            .any(|name| {
                name.starts_with('!') || name.contains(['*', '?']) || name.len() >= LONGEST_ELEMENT
            })
    }

    fn line(&self) -> String {
        let mut words = vec![self.principals.join(",")];
        let namespaces =
            (self.namespaces.as_ref()).map(|all| format!("namespaces=\"{}\"", all.join(",")));
        let options: Vec<String> = self.others.iter().cloned().chain(namespaces).collect();
        if !options.is_empty() {
            words.push(options.join(","));
        }
        words.push(self.key.clone());
        words.extend(self.comment.clone());
        words.join(" ")
    }
}

/// The length in bytes from which ssh-keygen's pattern matching refuses a
/// list element, and so matches nothing against the whole list.
const LONGEST_ELEMENT: usize = 1023;

/// Whether ssh-keygen splits an allowed signers line at `c`: a space or a
/// tab.
fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// `line` without the spaces, tabs and line ends ssh-keygen skips around it.
fn trim_blanks(line: &str) -> &str {
    line.trim_matches([' ', '\t', '\r', '\n'])
}

/// `text` split at each character `at` admits outside double quotes,
/// with the empty pieces between two such characters.
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
    pieces
}

/// A line of the printed file: Board's line as it was, or a key's line with
/// how it changed.
enum Printed {
    Kept(String),
    Key(Entry, Option<LineChange>, String),
}

/// The printed file as it is built, with the printed index and Board line
/// number of each key's lines.
#[derive(Default)]
struct Merge {
    printed: Vec<Printed>,
    at: std::collections::HashMap<String, Vec<(usize, usize)>>,
    duplicates: Vec<Duplicate>,
    left_out: Vec<LeftOut>,
    /// The keys lines of the attention file added or gained.
    given: std::collections::HashSet<String>,
}

/// `board`, the text of Board's allowed signers file, with each key of
/// `attention` admitted in the order namespace, such that no printed line
/// admits more than the lines it came from:
/// - two lines of Board's with one key, the same principals and the same
///   other options print as one with both lines' namespaces, or none when
///   either names none; other pairs print unchanged; both are reported;
/// - an attention line that cannot be read, that names patterns, that does
///   not admit `owner` in orders, or whose key an earlier line gave is left
///   out, as is one whose key Board lists on a line that names patterns or
///   on a line that already admits `owner` in orders;
/// - Board's line for a key such a line lists gains the order namespace
///   when it names `owner`, the attention line admits all its principals and
///   the other options are the same; otherwise it is left as it was and the
///   attention line left out;
/// - a key only in `attention` is added with that line's principals and
///   options, for orders alone.
fn merged_signers(board: &str, attention: &str) -> Signers {
    let mut merge = Merge::default();
    for (number, line) in (1..).zip(board.lines()) {
        merge.board_line(number, line);
    }
    for (number, line) in (1..).zip(attention.lines()) {
        merge.attention_line(number, line);
    }
    merge.finish()
}

impl Merge {
    fn board_line(&mut self, number: usize, line: &str) {
        let Some(entry) = Entry::parse(line) else {
            self.printed.push(Printed::Kept(line.to_owned()));
            return;
        };
        let seen = self.at.entry(entry.key.clone()).or_default();
        let same = (seen.iter()).find_map(|&(index, _)| match &self.printed[index] {
            Printed::Key(kept, ..)
                if kept.principals == entry.principals && kept.others == entry.others =>
            {
                Some(index)
            }
            _ => None,
        });
        let merged = same.is_some();
        if let Some(index) = same
            && let Printed::Key(kept, change, _) = &mut self.printed[index]
        {
            kept.namespaces =
                (kept.namespaces.take())
                    .zip(entry.namespaces.clone())
                    .map(|(mut all, more)| {
                        for namespace in more {
                            if !all.contains(&namespace) {
                                all.push(namespace);
                            }
                        }
                        all
                    });
            *change = Some(LineChange::Merged);
        } else {
            seen.push((self.printed.len(), number));
            self.printed
                .push(Printed::Key(entry.clone(), None, line.to_owned()));
        }
        if let Some(&(_, first)) = seen.first()
            && first != number
        {
            match (self.duplicates.iter_mut()).find(|found| found.key == entry.key) {
                Some(found) => {
                    found.lines.push(number);
                    found.merged &= merged;
                }
                None => self.duplicates.push(Duplicate {
                    key: entry.key,
                    lines: vec![first, number],
                    merged,
                }),
            }
        }
    }

    fn attention_line(&mut self, number: usize, line: &str) {
        let text = trim_blanks(line);
        if text.is_empty() || text.starts_with('#') {
            return;
        }
        let Some(entry) = Entry::parse(line) else {
            self.left_out.push(LeftOut {
                line: number,
                key: None,
                reason: LeftOutReason::Unreadable,
            });
            return;
        };
        if let Some(reason) = self.attention_entry(&entry) {
            self.left_out.push(LeftOut {
                line: number,
                key: Some(entry.key),
                reason,
            });
        }
    }

    /// Adds what the attention line `entry` gives, or why it gives nothing.
    fn attention_entry(&mut self, entry: &Entry) -> Option<LeftOutReason> {
        let namespace = board_model::ORDER_TYPE;
        if entry.has_pattern() {
            return Some(LeftOutReason::Pattern);
        }
        if !entry.principals.iter().any(|p| p == PRINCIPAL) {
            return Some(LeftOutReason::NotOwner);
        }
        if !entry.admits(PRINCIPAL, namespace) {
            return Some(LeftOutReason::NotOrders);
        }
        if self.given.contains(&entry.key) {
            return Some(LeftOutReason::Duplicate);
        }
        let Some(seen) = self.at.get(&entry.key) else {
            let added = Entry {
                namespaces: Some(vec![namespace.to_owned()]),
                ..entry.clone()
            };
            self.given.insert(added.key.clone());
            self.at
                .insert(added.key.clone(), vec![(self.printed.len(), 0)]);
            self.printed
                .push(Printed::Key(added, Some(LineChange::Added), String::new()));
            return None;
        };
        let lines: Vec<(usize, &Entry)> = (seen.iter())
            .filter_map(|&(index, _)| match &self.printed[index] {
                Printed::Key(kept, ..) => Some((index, kept)),
                Printed::Kept(_) => None,
            })
            .collect();
        if lines.iter().any(|(_, kept)| kept.has_pattern()) {
            return Some(LeftOutReason::Pattern);
        }
        if lines
            .iter()
            .any(|(_, kept)| kept.admits(PRINCIPAL, namespace))
        {
            return Some(LeftOutReason::AlreadyAdmitted);
        }
        let fits = lines.iter().find(|(_, kept)| {
            kept.others == entry.others
                && kept.principals.iter().any(|p| p == PRINCIPAL)
                && (kept.principals.iter()).all(|p| entry.principals.contains(p))
        });
        let Some(&(index, _)) = fits else {
            return Some(LeftOutReason::Differs);
        };
        if let Printed::Key(kept, change, _) = &mut self.printed[index]
            && let Some(namespaces) = &mut kept.namespaces
        {
            namespaces.push(namespace.to_owned());
            change.get_or_insert(LineChange::Gained);
        }
        self.given.insert(entry.key.clone());
        None
    }

    fn finish(self) -> Signers {
        let mut text = String::new();
        let mut changes = Vec::new();
        for (number, line) in (1..).zip(&self.printed) {
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
            duplicates: self.duplicates,
            left_out: self.left_out,
        }
    }
}

#[cfg(test)]
mod tests;
