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

/// The file whose presence marks a folder in a skills directory as Board's.
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
Each skill goes in <dir>/<name>/ with a .board-skill marker. Install
replaces a marked folder that differs and removes one for a retired skill.
A folder without the marker at a skill's name is refused with
board::skills::not_board, and nothing is written, unless --force replaces
it. Exits 1 only on a refusal or a failed write.

Examples:
  board skills install --dir skills
  board skills install --dir skills --force
  board skills install --dir skills --json")]
    Install {
        #[command(flatten)]
        at: At,
        /// Replace a folder at a skill's name that is not Board's; a symlink there is removed, never followed.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        page: Page,
    },
    /// List each skill this binary ships and each Board folder in a skills directory.
    #[command(after_help = "\
Each one's state is current (its files equal this binary's), stale (a
Board folder whose files differ), missing, not_board (a folder without the
.board-skill marker) or retired (a Board folder for a skill this binary no
longer ships). `board skills install` makes every one current. List changes
nothing and exits 0 unless the directory cannot be read.

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
    /// A directory holding the marker, with each other file's bytes by its
    /// path within the folder; `None` for an entry that is not a file.
    Board(BTreeMap<String, Option<Vec<u8>>>),
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
}

/// What `skills install` did with a skill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Installed {
    Written,
    Updated,
    Unchanged,
    Removed,
}

/// What `skills install` and `list` refuse.
#[derive(Debug)]
enum SkillsError {
    /// Folders at skills' names that are not Board's, without `--force`.
    NotBoard { folders: Vec<String> },
    /// A read or write of `path` failed.
    WriteFailed { path: PathBuf, source: io::Error },
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
                    "move each folder out of {dir}, or replace them with `board skills install --dir {dir} --force`"
                );
                let refusal = Refusal::new(Code::SkillsNotBoard, message, &Fields::default());
                let mut refusal = refusal.with_hint(hint);
                refusal.fields.insert("folders".to_owned(), json!(folders));
                refusal
            }
            Self::WriteFailed { path, source } => {
                let message = format!("cannot write {}: {source}", path.display());
                let mut refusal =
                    Refusal::new(Code::SkillsWriteFailed, message, &Fields::default());
                let file = path.display().to_string();
                refusal.fields.insert("file".to_owned(), json!(file));
                refusal
            }
        };
        refusal.fields.insert("dir".to_owned(), json!(dir));
        refusal
    }
}

/// `path`'s failure as a [`SkillsError`].
fn failed(path: &Path) -> impl FnOnce(io::Error) -> SkillsError + '_ {
    move |source| SkillsError::WriteFailed {
        path: path.to_path_buf(),
        source,
    }
}

/// `board skills <command>`.
pub(crate) fn skills(command: &SkillsCommand, context: &Context, printer: Printer) -> Output {
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
    let held = match read(&dir) {
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
/// with its state, in name order.
fn listed(skills: &[Skill], held: &BTreeMap<String, Held>) -> Vec<(String, Listed)> {
    let mut listed: BTreeMap<String, Listed> = held
        .iter()
        .filter(|(_, held)| matches!(held, Held::Board(_)))
        .map(|(name, _)| (name.clone(), Listed::Retired))
        .collect();
    for skill in skills {
        let state = match held.get(skill.name) {
            None => Listed::Missing,
            Some(Held::Foreign) => Listed::NotBoard,
            Some(Held::Board(found)) if *found == files(skill) => Listed::Current,
            Some(Held::Board(_)) => Listed::Stale,
        };
        listed.insert(skill.name.to_owned(), state);
    }
    listed.into_iter().collect()
}

/// Installs `skills` into `dir`, which held `held`: refuses every folder
/// at a skill's name that is not Board's unless `force`, then removes each
/// retired Board folder and writes each skill whose folder differs.
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
        }
    }
    for skill in skills {
        let state = listed.iter().find(|(name, _)| name == skill.name);
        let installed = match state.map(|(_, state)| *state) {
            Some(Listed::Current) => Installed::Unchanged,
            Some(Listed::Stale) => Installed::Updated,
            _ => Installed::Written,
        };
        if installed != Installed::Unchanged {
            put(dir, skill)?;
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
        entries => entries.map_err(failed(dir))?,
    };
    let mut held = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(failed(dir))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = entry.path();
        let kind = std::fs::symlink_metadata(&path).map_err(failed(&path))?;
        let marked =
            std::fs::symlink_metadata(path.join(MARKER)).is_ok_and(|marker| marker.is_file());
        let found = if kind.is_dir() && marked {
            let mut files = BTreeMap::new();
            folder(&path, "", &mut files)?;
            Held::Board(files)
        } else {
            Held::Foreign
        };
        held.insert(name, found);
    }
    Ok(held)
}

/// Each entry beneath `directory`, whose path within the folder starts
/// with `within`, into `files`, but the marker at the folder's top.
fn folder(
    directory: &Path,
    within: &str,
    files: &mut BTreeMap<String, Option<Vec<u8>>>,
) -> Result<(), SkillsError> {
    for entry in std::fs::read_dir(directory).map_err(failed(directory))? {
        let entry = entry.map_err(failed(directory))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = format!("{within}{name}");
        let kind = std::fs::symlink_metadata(&path).map_err(failed(&path))?;
        if relative == MARKER {
            continue;
        }
        if kind.is_dir() {
            folder(&path, &format!("{relative}/"), files)?;
        } else if kind.is_file() {
            let bytes = std::fs::read(&path).map_err(failed(&path))?;
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
    std::fs::create_dir_all(dir).map_err(failed(dir))
}

/// Removes the folder or symlink at `path`; a symlink is removed itself,
/// never followed.
#[expect(
    clippy::disallowed_methods,
    reason = "this module owns the writes to a skills directory, which is no board's tree"
)]
fn remove(path: &Path) -> Result<(), SkillsError> {
    let kind = std::fs::symlink_metadata(path).map_err(failed(path))?;
    if kind.is_dir() {
        std::fs::remove_dir_all(path).map_err(failed(path))
    } else {
        std::fs::remove_file(path).map_err(failed(path))
    }
}

/// Writes `skill` into a new folder beside its place in `dir`, marker
/// first, then swaps it into place, so a reader never sees half a skill.
/// Whatever stood there is moved aside and removed: a symlink itself,
/// never what it points to.
#[expect(
    clippy::disallowed_methods,
    reason = "this module owns the writes to a skills directory, which is no board's tree"
)]
fn put(dir: &Path, skill: &Skill) -> Result<(), SkillsError> {
    let target = dir.join(skill.name);
    let new = dir.join(format!(".{}.board-new", skill.name));
    std::fs::create_dir(&new).map_err(failed(&new))?;
    let marker = format!("{BINARY_NAME} {}\n", crate::timing::BUILD);
    let files = std::iter::once((MARKER, marker.as_bytes())).chain(skill.files.iter().copied());
    for (path, bytes) in files {
        let path = new.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(failed(parent))?;
        }
        std::fs::write(&path, bytes).map_err(failed(&path))?;
    }
    if std::fs::symlink_metadata(&target).is_err() {
        return std::fs::rename(&new, &target).map_err(failed(&target));
    }
    let old = dir.join(format!(".{}.board-old", skill.name));
    std::fs::rename(&target, &old).map_err(failed(&target))?;
    std::fs::rename(&new, &target).map_err(failed(&target))?;
    remove(&old)
}

/// A skill or Board folder as `skills list` and `skills install` print it.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Skilled<S> {
    /// The skill's name, which names its folder.
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
    "What install wrote, updated, left unchanged or removed in a skills directory.",
)
.when_empty("this binary ships no skills and the directory held no Board folder");

pub(crate) const LIST: Leaf = Leaf::new(&["skills", "list"]).document(LIST_REPORT);
pub(crate) const INSTALL: Leaf = Leaf::new(&["skills", "install"]).document(INSTALL_REPORT);

#[cfg(test)]
mod tests;
