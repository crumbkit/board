//! `board skills install` and `list`: the skills built into this binary
//! (docs/plugins.md, Skills), and the one module that writes a skills
//! directory.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use board_model::{BINARY_NAME, Code, Fields, Refusal};
use clap::Subcommand;
use crumbkit::help::Leaf;
use crumbkit::{Document, Listing, Row};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

use crate::config::Context;
use crate::render::{self, Printer};
use crate::report::Output;
use crate::run::{Page, plain};

/// A skill built into this binary: its name and each file's path within
/// its folder, with the file's bytes.
pub(crate) struct Skill {
    pub(crate) name: &'static str,
    pub(crate) files: &'static [(&'static str, &'static [u8])],
}

/// The skills of the plugins this binary is built with, in name order,
/// which the build script finds and embeds.
pub(crate) const SKILLS: &[Skill] = include!(concat!(env!("OUT_DIR"), "/skills.rs"));

/// The hidden folder in a skills directory to which `--force` moves a
/// folder that is not Board's, as `<name>-<time>`. Agents load the folders
/// one level down in a skills directory, so they never load a backup, and
/// list and install pass over this folder.
const BACKUPS: &str = ".board-backups";

/// The file whose presence marks a folder in a skills directory as Board's.
/// It holds one line, `board <version> <skill>`, naming the version that
/// wrote it and the skill it holds.
pub(crate) const MARKER: &str = ".board-skill";

/// `board skills -h`'s examples.
pub(crate) const HELP: &str = "\
A skills directory, ~/.claude/skills unless --dir names another, holds one
folder per skill. Run install after each upgrade of Board.

Examples:
  board skills install --dir skills
  board skills list --dir skills";

/// Where `--dir` is left out: the skills directory in the home directory.
const DEFAULT: &str = ".claude/skills";

#[derive(Debug, Subcommand)]
pub(crate) enum SkillsCommand {
    /// Write the skills built into this binary into a skills directory.
    #[command(after_help = "\
Each skill goes in <dir>/<name>/ with a .board-skill marker naming it.
Install replaces a Board folder that differs, removes a retired one, and
keeps the rest. It refuses a folder at a skill's name that is not Board's
with board::skills::not_board and writes nothing, unless --force. Exits 1
on a refusal or failed write.

Examples:
  board skills install --dir skills
  board skills install --dir skills --force
  board skills install --dir skills --json")]
    Install {
        #[command(flatten)]
        at: At,
        /// Move a folder at a skill's name that is not Board's to a backup.
        #[arg(
            long,
            help = "Move a folder or symlink at a skill's name that is not Board's to <dir>/.board-backups/<name>-<time>, which agents do not load"
        )]
        force: bool,
        #[command(flatten)]
        page: Page,
    },
    /// List each skill this binary ships and each Board folder in a skills directory.
    #[command(after_help = "\
Each one's state is current (its files equal this binary's), stale (a
Board folder whose files differ), missing, not_board (a folder without the
.board-skill marker, or whose marker does not name it), retired (a Board
folder, named in its own marker, for a skill this binary no longer ships)
or copy (a marked folder whose marker names another skill or none, which
install keeps). `board skills install` makes each
shipped skill current. List changes nothing and exits 0 unless the
directory cannot be read.

Examples:
  board skills list --dir skills
  board skills list --dir skills --json")]
    List {
        #[command(flatten)]
        at: At,
        #[command(flatten)]
        page: Page,
    },
}

/// The skills directory a command works in.
#[derive(Debug, clap::Args)]
pub(crate) struct At {
    /// The skills directory, such as ~/.agents/skills; ~/.claude/skills when left out.
    #[arg(long, value_name = "DIR", value_hint = clap::ValueHint::DirPath)]
    dir: Option<PathBuf>,
}

/// What a skills directory holds at one name.
#[derive(Debug, PartialEq, Eq)]
enum Held {
    /// A directory holding the marker: the skill the marker names, if it
    /// names one, and each other file's bytes by its path within the
    /// folder, `None` for an entry that is not a file.
    Board {
        skill: Option<String>,
        files: BTreeMap<String, Option<Vec<u8>>>,
    },
    /// A file, a symlink, or a directory without the marker.
    Foreign,
}

/// A skill's state in `skills list`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Listed {
    Current,
    Stale,
    Missing,
    NotBoard,
    Retired,
    Copy,
}

/// What `skills install` did with a skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Installed {
    Written,
    Updated,
    Unchanged,
    Removed,
    Kept,
    BackedUp,
}

/// What `skills install` and `list` refuse.
#[derive(Debug)]
enum SkillsError {
    /// Folders at skills' names that are not Board's, without `--force`.
    NotBoard { folders: Vec<String> },
    /// A read, write or move of `path` failed. `stranded` is the backup
    /// that holds a folder `--force` moved aside and could not move back.
    WriteFailed {
        path: PathBuf,
        act: Act,
        source: io::Error,
        stranded: Option<PathBuf>,
    },
}

/// What failed on a path.
#[derive(Debug)]
enum Act {
    Read,
    Write,
    /// A move to the path this names.
    Move(PathBuf),
}

impl SkillsError {
    /// The refusal, naming the skills directory as given, `shown`.
    fn refusal(&self, shown: &Path) -> Refusal {
        let dir = shown.display().to_string();
        let mut refusal = match self {
            Self::NotBoard { folders } => {
                let message = format!(
                    "{dir} holds {} without a {MARKER} file, so install will not replace {}",
                    folders.join(", "),
                    if folders.len() == 1 { "it" } else { "them" },
                );
                let hint = format!(
                    "move each folder out of {dir}, or move them aside to a backup with `board skills install --dir {} --force`",
                    quote(shown)
                );
                let refusal = Refusal::new(Code::SkillsNotBoard, message, &Fields::default());
                let mut refusal = refusal.with_hint(hint);
                refusal.fields.insert("folders".to_owned(), json!(folders));
                refusal
            }
            Self::WriteFailed {
                path,
                act,
                source,
                stranded,
            } => {
                let path_shown = path.display();
                let mut message = match act {
                    Act::Read => format!("cannot read {path_shown}: {source}"),
                    Act::Write => format!("cannot write {path_shown}: {source}"),
                    Act::Move(to) => {
                        format!("cannot move {path_shown} to {}: {source}", to.display())
                    }
                };
                if let Some(backup) = stranded {
                    let backup = backup.display();
                    message = format!("{message}, and the folder that stood there is at {backup}");
                }
                let mut refusal =
                    Refusal::new(Code::SkillsWriteFailed, message, &Fields::default());
                let file = path.display().to_string();
                refusal.fields.insert("file".to_owned(), json!(file));
                if let Some(backup) = stranded {
                    let backup = backup.display().to_string();
                    refusal.fields.insert("backup".to_owned(), json!(backup));
                }
                refusal
            }
        };
        refusal.fields.insert("dir".to_owned(), json!(dir));
        refusal
    }
}

/// A failed read of `path` as a [`SkillsError`].
fn unread(path: &Path) -> impl FnOnce(io::Error) -> SkillsError + '_ {
    failed(path, Act::Read)
}

/// A failed write of `path` as a [`SkillsError`].
fn unwritten(path: &Path) -> impl FnOnce(io::Error) -> SkillsError + '_ {
    failed(path, Act::Write)
}

/// A failed `act` on `path` as a [`SkillsError`].
fn failed(path: &Path, act: Act) -> impl FnOnce(io::Error) -> SkillsError + '_ {
    move |source| SkillsError::WriteFailed {
        path: path.to_path_buf(),
        act,
        source,
        stranded: None,
    }
}

/// `path` as one POSIX shell word: as it is when no character needs
/// quoting, else in single quotes.
fn quote(path: &Path) -> String {
    let text = path.display().to_string();
    let safe =
        |character: char| character.is_ascii_alphanumeric() || "@%+=:,./-_~".contains(character);
    if !text.is_empty() && text.chars().all(safe) {
        return text;
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `board skills <command>`.
/// `elsewhere` is set when `-C` or `--personal` names a board, which a
/// skills directory is not, so the command line is refused.
pub(crate) fn skills(
    command: &SkillsCommand,
    context: &Context,
    printer: Printer,
    chose: bool,
) -> Output {
    if chose {
        let message = "board skills takes no -C or --personal, which choose a board";
        let refusal = plain(Code::CliUsage, &message);
        return render::refused(
            printer,
            refusal.with_hint("name the skills directory with `--dir`"),
        );
    }
    let (SkillsCommand::Install { at, page, .. } | SkillsCommand::List { at, page }) = command;
    let Some(shown) = at
        .dir
        .clone()
        .or_else(|| context.home.as_ref().map(|home| home.join(DEFAULT)))
    else {
        let refusal = plain(
            Code::CliUsage,
            &"no home directory is known, so there is no default skills directory",
        )
        .with_hint("name the skills directory with `--dir`");
        return render::refused(printer, refusal);
    };
    let dir = context.start.join(&shown);
    let held = match read(&dir).map(|held| keyed(held, SKILLS)) {
        Ok(held) => held,
        Err(error) => return render::refused(printer, error.refusal(&shown)),
    };
    match command {
        SkillsCommand::List { .. } => {
            let rows = listed(SKILLS, &held).into_iter();
            let rows = rows.map(|(name, state)| Skilled::new(&shown, name, state));
            printer.print(
                LIST_REPORT,
                Ok(page.first_of(rows.collect()).described("skills")),
            )
        }
        SkillsCommand::Install { force, .. } => match install(&dir, SKILLS, &held, *force) {
            Ok(done) => {
                let rows = done.into_iter();
                let rows = rows.map(|(name, state)| Skilled::new(&shown, name, state));
                printer.print(
                    INSTALL_REPORT,
                    Ok(page.first_of(rows.collect()).described("skills")),
                )
            }
            Err(error) => render::refused(printer, error.refusal(&shown)),
        },
    }
}

/// `skills`' files as `Held::Board` reads a folder.
fn files(skill: &Skill) -> BTreeMap<String, Option<Vec<u8>>> {
    let files = skill.files.iter();
    files
        .map(|(path, bytes)| ((*path).to_owned(), Some(bytes.to_vec())))
        .collect()
}

/// Each skill in `skills` and each Board folder in `held` for no skill,
/// with its state, in name order. A Board folder for no skill is retired
/// only when its marker names the skill its folder is named for; one whose
/// marker names another skill, or none, is a copy. At a skill's name, a
/// folder whose marker does not name that skill is not Board's.
fn listed(skills: &[Skill], held: &BTreeMap<String, Held>) -> Vec<(String, Listed)> {
    let mut listed: BTreeMap<String, Listed> = held
        .iter()
        .filter_map(|(name, held)| match held {
            Held::Board {
                skill: Some(skill), ..
            } if skill == name => Some((name.clone(), Listed::Retired)),
            Held::Board { .. } => Some((name.clone(), Listed::Copy)),
            Held::Foreign => None,
        })
        .collect();
    for skill in skills {
        let state = match held.get(skill.name) {
            None => Listed::Missing,
            Some(Held::Foreign) => Listed::NotBoard,
            Some(Held::Board { skill: holds, .. }) if holds.as_deref() != Some(skill.name) => {
                Listed::NotBoard
            }
            Some(Held::Board { files: found, .. }) if *found == files(skill) => Listed::Current,
            Some(Held::Board { .. }) => Listed::Stale,
        };
        listed.insert(skill.name.to_owned(), state);
    }
    listed.into_iter().collect()
}

/// Installs `skills` into `dir`, which held `held`: refuses every folder
/// at a skill's name that is not Board's unless `force`, then removes each
/// retired Board folder, keeps each copy, and writes each skill whose
/// folder differs, moving a folder that is not Board's aside to a backup.
fn install(
    dir: &Path,
    skills: &[Skill],
    held: &BTreeMap<String, Held>,
    force: bool,
) -> Result<Vec<(String, Installed)>, SkillsError> {
    let listed = listed(skills, held);
    let foreign: Vec<String> = (listed.iter())
        .filter(|(_, state)| *state == Listed::NotBoard)
        .map(|(name, _)| name.clone())
        .collect();
    if !foreign.is_empty() && !force {
        return Err(SkillsError::NotBoard { folders: foreign });
    }
    create(dir)?;
    let mut done = Vec::new();
    for (name, state) in &listed {
        if *state == Listed::Retired {
            remove(&dir.join(name))?;
            done.push((name.clone(), Installed::Removed));
        } else if *state == Listed::Copy {
            done.push((name.clone(), Installed::Kept));
        }
    }
    for skill in skills {
        let state = listed.iter().find(|(name, _)| name == skill.name);
        let installed = match state.map(|(_, state)| *state) {
            Some(Listed::Current) => Installed::Unchanged,
            Some(Listed::Stale) => Installed::Updated,
            _ => Installed::Written,
        };
        if let Some((_, state)) = state.filter(|_| installed != Installed::Unchanged)
            && let Some(backup) = put(dir, skill, *state)?
        {
            let name = backup.file_name().unwrap_or_default().to_string_lossy();
            done.push((format!("{BACKUPS}/{name}"), Installed::BackedUp));
        }
        done.push((skill.name.to_owned(), installed));
    }
    done.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(done)
}

/// What `dir` holds at each name, or nothing when it does not exist.
fn read(dir: &Path) -> Result<BTreeMap<String, Held>, SkillsError> {
    let entries = match std::fs::read_dir(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        entries => entries.map_err(unread(dir))?,
    };
    let mut held = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(unread(dir))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let scratch = [".board-new", ".board-old"]
            .iter()
            .any(|end| name.ends_with(end));
        if (name.starts_with('.') && scratch) || name == BACKUPS {
            continue;
        }
        let path = entry.path();
        let found = if let Some(marker) = marker(&path) {
            let mut files = BTreeMap::new();
            folder(&path, "", &mut files)?;
            let skill = named(&marker);
            Held::Board { skill, files }
        } else {
            Held::Foreign
        };
        held.insert(name, found);
    }
    Ok(held)
}

/// `raw` with each entry named as a skill in other letter case, such as
/// `Changes`, keyed by that skill's name when no entry has its exact name,
/// so a case-insensitive file system's folder is never taken for a missing
/// one.
fn keyed(raw: BTreeMap<String, Held>, skills: &[Skill]) -> BTreeMap<String, Held> {
    let names: Vec<&str> = skills.iter().map(|skill| skill.name).collect();
    let mut held = BTreeMap::new();
    for (name, found) in raw {
        let lower = name.to_lowercase();
        let skill = names.iter().find(|skill| **skill == lower);
        match skill {
            Some(skill) if **skill != name && !held.contains_key(*skill) => {
                held.insert((*skill).to_owned(), found);
            }
            _ => {
                held.insert(name, found);
            }
        }
    }
    held
}

/// The longest marker read: one line is far shorter.
const MARKER_BYTES: u64 = 1024;

/// The text of the marker in `path`, when `path` is a directory, never a
/// symlink, holding a marker that is a regular file, never a symlink, a
/// FIFO or a device, which can be read. Only its first [`MARKER_BYTES`]
/// are read.
fn marker(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let directory = std::fs::symlink_metadata(path).is_ok_and(|kind| kind.is_dir());
    let marker = path.join(MARKER);
    let file = directory && std::fs::symlink_metadata(&marker).is_ok_and(|kind| kind.is_file());
    if !file {
        return None;
    }
    let mut bytes = Vec::new();
    let opened = std::fs::File::open(&marker).ok()?;
    opened.take(MARKER_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The skill a marker's text names, its third word, or `None` for a
/// marker that names none.
fn named(marker: &str) -> Option<String> {
    marker.split_whitespace().nth(2).map(str::to_owned)
}

/// Each entry beneath `directory`, whose path within the folder starts
/// with `within`, into `files`, but the marker at the folder's top.
fn folder(
    directory: &Path,
    within: &str,
    files: &mut BTreeMap<String, Option<Vec<u8>>>,
) -> Result<(), SkillsError> {
    for entry in std::fs::read_dir(directory).map_err(unread(directory))? {
        let entry = entry.map_err(unread(directory))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{within}{name}");
        let kind = std::fs::symlink_metadata(&path).map_err(unread(&path))?;
        if relative == MARKER {
            continue;
        }
        if kind.is_dir() {
            folder(&path, &format!("{relative}/"), files)?;
        } else if kind.is_file() {
            let bytes = std::fs::read(&path).map_err(unread(&path))?;
            files.insert(relative, Some(bytes));
        } else {
            files.insert(relative, None);
        }
    }
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "this module owns the writes to a skills directory, which is no board's tree"
)]
fn create(dir: &Path) -> Result<(), SkillsError> {
    std::fs::create_dir_all(dir).map_err(unwritten(dir))
}

/// Removes the folder or symlink at `path`; a symlink is removed itself,
/// never followed.
#[expect(
    clippy::disallowed_methods,
    reason = "this module owns the writes to a skills directory, which is no board's tree"
)]
fn remove(path: &Path) -> Result<(), SkillsError> {
    let kind = std::fs::symlink_metadata(path).map_err(unread(path))?;
    if kind.is_dir() {
        std::fs::remove_dir_all(path).map_err(unwritten(path))
    } else {
        std::fs::remove_file(path).map_err(unwritten(path))
    }
}

/// Writes `skill` into a new folder beside its place in `dir`, marker
/// first, then swaps it into place, so a reader never sees half a skill.
/// A leftover at either scratch name is removed first. The folder install
/// found in `state` is checked again just before the swap: a missing
/// folder must still be missing, and a stale one must still be Board's,
/// with a marker that names this skill.
/// A stale folder that stood there is moved aside and removed; a folder
/// or symlink that is not Board's is moved to a backup in [`BACKUPS`],
/// whose path this returns. A symlink is moved itself, never followed.
fn put(dir: &Path, skill: &Skill, state: Listed) -> Result<Option<PathBuf>, SkillsError> {
    let new = dir.join(format!(".{}.board-new", skill.name));
    let old = dir.join(format!(".{}.board-old", skill.name));
    for scratch in [&new, &old] {
        if std::fs::symlink_metadata(scratch).is_ok() {
            remove(scratch)?;
        }
    }
    let written = fill(&new, skill).and_then(|()| swap(dir, skill.name, &new, &old, state));
    if written.is_err() && std::fs::symlink_metadata(&new).is_ok() {
        // Best effort: the error that stopped the write is the one to report.
        let _ = remove(&new);
    }
    written
}

/// Writes the marker and then `skill`'s files into the new folder `new`.
#[expect(
    clippy::disallowed_methods,
    reason = "this module owns the writes to a skills directory, which is no board's tree"
)]
fn fill(new: &Path, skill: &Skill) -> Result<(), SkillsError> {
    std::fs::create_dir(new).map_err(unwritten(new))?;
    let marker = format!("{BINARY_NAME} {} {}\n", crate::timing::BUILD, skill.name);
    let files = std::iter::once((MARKER, marker.as_bytes())).chain(skill.files.iter().copied());
    for (path, bytes) in files {
        let path = new.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(unwritten(parent))?;
        }
        std::fs::write(&path, bytes).map_err(unwritten(&path))?;
    }
    Ok(())
}

/// Moves `new` to `name` in `dir`, unless what stands there is no longer
/// what `state` found. What stands there is moved to a backup when it is
/// not Board's, whose path this returns, and moved back when `new` cannot
/// take its place; otherwise it is moved to `old` and removed.
#[expect(
    clippy::disallowed_methods,
    reason = "this module owns the writes to a skills directory, which is no board's tree"
)]
fn swap(
    dir: &Path,
    name: &str,
    new: &Path,
    old: &Path,
    state: Listed,
) -> Result<Option<PathBuf>, SkillsError> {
    let target = dir.join(name);
    let present = std::fs::symlink_metadata(&target).is_ok();
    let changed = match state {
        Listed::Missing => present,
        Listed::Stale => {
            marker(&target).is_none_or(|marker| named(&marker).as_deref() != Some(name))
        }
        Listed::Current | Listed::NotBoard | Listed::Retired | Listed::Copy => false,
    };
    if changed {
        let folders = vec![name.to_owned()];
        return Err(SkillsError::NotBoard { folders });
    }
    let into = |from: &Path, to: &Path| {
        std::fs::rename(from, to).map_err(failed(from, Act::Move(to.to_path_buf())))
    };
    if !present {
        return into(new, &target).map(|()| None);
    }
    if state == Listed::NotBoard {
        let stamp = jiff::Timestamp::now()
            .strftime("%Y%m%dT%H%M%SZ")
            .to_string();
        create(&dir.join(BACKUPS))?;
        let backup = backup(dir, name, &stamp);
        into(&target, &backup)?;
        return match into(new, &target) {
            Ok(()) => Ok(Some(backup)),
            Err(error) if std::fs::rename(&backup, &target).is_ok() => Err(error),
            Err(SkillsError::WriteFailed {
                path, act, source, ..
            }) => Err(SkillsError::WriteFailed {
                path,
                act,
                source,
                stranded: Some(backup),
            }),
            Err(error) => Err(error),
        };
    }
    into(&target, old)?;
    into(new, &target)?;
    remove(old).map(|()| None)
}

/// Where `--force` moves the folder at `name` in `dir`: `<name>-<stamp>`
/// in [`BACKUPS`], or the first of it with `-2`, `-3` and on after it that
/// nothing holds.
fn backup(dir: &Path, name: &str, stamp: &str) -> PathBuf {
    let backups = dir.join(BACKUPS);
    let base = format!("{name}-{stamp}");
    let free = |path: &PathBuf| std::fs::symlink_metadata(path).is_err();
    let first = backups.join(&base);
    if free(&first) {
        return first;
    }
    let mut later = (2_u64..).map(|next| backups.join(format!("{base}-{next}")));
    later.find(free).unwrap_or(first)
}

/// A skill or Board folder as `skills list` and `skills install` print it.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Skilled<S> {
    /// The skill's name, which names its folder, or a backup's path within
    /// the skills directory.
    name: String,
    /// The folder's state.
    state: S,
    /// The folder, in the skills directory as given.
    path: String,
}

impl<S> Skilled<S> {
    fn new(dir: &Path, name: String, state: S) -> Self {
        let path = dir.join(&name).display().to_string();
        Self { name, state, path }
    }
}

impl<S: Serialize + JsonSchema> Row for Skilled<S> {
    const COLUMNS: &'static [&'static str] = &["state", "name", "path"];
}

const LIST_REPORT: Document<Listing<Skilled<Listed>>> = Document::new(
    "board",
    "skills-list-report",
    2,
    "Each skill this binary ships and each Board folder in a skills directory, with its state.",
)
.when_empty("this binary ships no skills and the directory holds no Board folder");

const INSTALL_REPORT: Document<Listing<Skilled<Installed>>> = Document::new(
    "board",
    "skills-install-report",
    2,
    "What install wrote, updated, left unchanged, removed, kept or backed up in a skills directory.",
)
.when_empty("this binary ships no skills and the directory held no Board folder");

pub(crate) const LIST: Leaf = Leaf::new(&["skills", "list"]).document(LIST_REPORT);
pub(crate) const INSTALL: Leaf = Leaf::new(&["skills", "install"]).document(INSTALL_REPORT);

#[cfg(test)]
mod tests;
