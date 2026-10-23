//! Following a git symlink to the file it names inside its own tree
//! (docs/architecture/paths/symlinks.md). Only the tree is read: a target
//! outside it, or an absolute one, which would name a file on the machine
//! reading the board, is refused.

use std::collections::HashMap;
use std::fmt;

use board_model::{Code, MAX_SYMLINKS, ObjectId};
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::git::GitError;
use crate::tree::{Kind, RawEntry, Tree};

/// The longest symlink text Board follows, in bytes. No system Board runs
/// on can make a longer symlink, so git cannot check one out, and the limit
/// bounds the work one text costs.
pub(crate) const MAX_TEXT_BYTES: usize = 4096;

/// A place in a tree that following has reached, as [`Chains`] numbers it.
pub(crate) type Node = usize;

/// The root of the tree.
pub(crate) const ROOT: Node = 0;

/// Each place in one tree that following symlinks has reached, and where
/// each symlink there leads. A chain that many symlinks reach is followed
/// once, each name is looked up in its directory once, and a step from a
/// place to a name costs the same however deep the place is.
#[derive(Debug)]
pub(crate) struct Chains<D> {
    places: Vec<Place<D>>,
    /// What following each symlink found; one not followed yet is absent.
    known: HashMap<Node, Known>,
    /// How many names of texts were stepped through. Work-count tests
    /// assert it.
    pub(crate) steps: usize,
}

impl<D> Default for Chains<D> {
    fn default() -> Self {
        Self {
            places: Vec::new(),
            known: HashMap::new(),
            steps: 0,
        }
    }
}

/// One place: the place of its directory, its name there, and what it is.
#[derive(Debug)]
struct Place<D> {
    parent: Node,
    name: Vec<u8>,
    what: What<D>,
}

#[derive(Debug)]
enum What<D> {
    /// A directory, with the place of each name looked up in it.
    Directory(D, HashMap<Vec<u8>, Node>),
    /// A file, with its blob and size.
    File(ObjectId, Option<u64>),
    /// A symlink, with its text, `None` when it cannot be read.
    Link(Option<Vec<u8>>),
    /// A symlink whose text is longer than [`MAX_TEXT_BYTES`], not loaded.
    Long,
    /// A file or directory that cannot be read.
    Unreadable,
    /// No entry, or a submodule, which holds no file of the tree.
    Nothing,
}

/// What following one symlink found.
#[derive(Clone, Debug)]
enum Known {
    /// It is being followed, so reaching it again is a loop.
    Open,
    /// Where it ends, after how many texts, its own included.
    Ended(End, usize),
    /// It takes more than this many texts.
    Over(usize),
}

/// Where following a symlink ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The file at `at`, with its blob and size.
    File {
        at: Node,
        blob: ObjectId,
        size: Option<u64>,
    },
    /// A directory, which is not followed: the symlink stays a file
    /// holding its text.
    Directory(Node),
    /// Why it leads to no file.
    Refused(Code),
}

/// Where a text leads, after how many texts, or `None` when it takes more
/// than it was allowed.
type Taken = Option<(End, usize)>;

impl<D: Clone> Chains<D> {
    /// The place of `entry`, in the directory at `at`, made from it the
    /// first time; a symlink's text is read then.
    pub(crate) fn enter<T: Tree<Directory = D>>(
        &mut self,
        tree: &mut T,
        at: Node,
        entry: &RawEntry<D>,
    ) -> Node {
        self.root(tree);
        let What::Directory(directory, names) = &self.places[at].what else {
            return self.add(at, entry.name.clone(), What::Nothing);
        };
        if let Some(place) = names.get(&entry.name) {
            return *place;
        }
        let directory = directory.clone();
        let entry = tree.settle(&directory, entry);
        let what = match entry.kind {
            Kind::Directory(child) => What::Directory(child, HashMap::new()),
            Kind::File(blob) => What::File(blob, entry.size),
            Kind::Symlink(_) => text(tree, &directory, &entry),
            Kind::Unreadable => What::Unreadable,
            Kind::Submodule | Kind::Unhashed(_) => What::Nothing,
        };
        self.add(at, entry.name, what)
    }

    /// The place at the `/`-separated physical `path`, found by exact name
    /// without following a symlink.
    pub(crate) fn find<T: Tree<Directory = D>>(
        &mut self,
        tree: &mut T,
        path: &[u8],
    ) -> Result<Node, GitError> {
        self.root(tree);
        let mut at = ROOT;
        for name in path
            .split(|byte| *byte == b'/')
            .filter(|name| !name.is_empty())
        {
            at = self.step(tree, at, name)?;
        }
        Ok(at)
    }

    /// Where the symlink at `link` leads, through any symlinks on the way,
    /// up to [`MAX_SYMLINKS`] in all, and its text as [`shown`] shows it;
    /// `None` when no symlink is there. A text that is empty, longer than
    /// [`MAX_TEXT_BYTES`] or not UTF-8 dangles: no system Board runs on
    /// makes the first two, and Board cannot name the file the third names,
    /// since it shows such a name lossily.
    pub(crate) fn follow<T: Tree<Directory = D>>(
        &mut self,
        tree: &mut T,
        link: Node,
    ) -> Result<Option<(End, String)>, GitError> {
        let text = match &self.places[link].what {
            What::Link(text) => text.clone(),
            What::Long => return Ok(Some((End::Refused(Code::SymlinkDangling), shown(None)))),
            _ => return Ok(None),
        };
        let shown = shown(Some(text.as_deref().unwrap_or_default()));
        let taken = self.chained(tree, link, text.as_deref(), MAX_SYMLINKS)?;
        let end = taken.map_or(End::Refused(Code::SymlinkLoop), |(end, _)| end);
        Ok(Some((end, shown)))
    }

    /// The bytes of the file at `at`, a place following reached, as `tree`
    /// reads it.
    pub(crate) fn read<T: Tree<Directory = D>>(
        &self,
        tree: &mut T,
        at: Node,
    ) -> Result<Option<Vec<u8>>, GitError> {
        let Place { parent, name, .. } = &self.places[at];
        let What::Directory(directory, _) = &self.places[*parent].what else {
            unreachable!("a file is reached in a directory");
        };
        let alike = tree.alike(directory, name, None)?;
        let entry = alike.iter().find(|entry| &entry.name == name);
        entry.map_or(Ok(None), |entry| tree.read(directory, entry))
    }

    /// The names from the root to `at`.
    pub(crate) fn names(&self, mut at: Node) -> Vec<&[u8]> {
        let mut names = Vec::new();
        while at != ROOT {
            names.push(self.places[at].name.as_slice());
            at = self.places[at].parent;
        }
        names.reverse();
        names
    }

    /// Makes the root's place, the first time.
    fn root<T: Tree<Directory = D>>(&mut self, tree: &T) {
        if self.places.is_empty() {
            let what = What::Directory(tree.root(), HashMap::new());
            self.places.push(Place {
                parent: ROOT,
                name: Vec::new(),
                what,
            });
        }
    }

    fn add(&mut self, parent: Node, name: Vec<u8>, what: What<D>) -> Node {
        let place = self.places.len();
        if let What::Directory(_, names) = &mut self.places[parent].what {
            names.insert(name.clone(), place);
        }
        self.places.push(Place { parent, name, what });
        place
    }

    /// The place `name` names in the directory at `at`, looked up in `tree`
    /// the first time.
    fn step<T: Tree<Directory = D>>(
        &mut self,
        tree: &mut T,
        at: Node,
        name: &[u8],
    ) -> Result<Node, GitError> {
        self.steps += 1;
        let What::Directory(directory, names) = &self.places[at].what else {
            return Ok(self.add(at, name.to_vec(), What::Nothing));
        };
        if let Some(place) = names.get(name) {
            return Ok(*place);
        }
        let directory = directory.clone();
        let alike = tree.alike(&directory, name, None)?;
        Ok(match alike.iter().find(|entry| entry.name == name) {
            Some(entry) => self.enter(tree, at, entry),
            None => self.add(at, name.to_vec(), What::Nothing),
        })
    }

    /// Follows the symlink at `link`, whose text is `text`, within
    /// `budget` texts, and remembers what it found.
    fn chained<T: Tree<Directory = D>>(
        &mut self,
        tree: &mut T,
        link: Node,
        text: Option<&[u8]>,
        budget: usize,
    ) -> Result<Taken, GitError> {
        match self.known.get(&link) {
            Some(Known::Open | Known::Ended(End::Refused(Code::SymlinkLoop), _)) => {
                return Ok(Some(refusal(Code::SymlinkLoop, 0)));
            }
            Some(Known::Ended(end, taken)) => {
                return Ok((*taken <= budget).then(|| (end.clone(), *taken)));
            }
            Some(Known::Over(over)) if *over >= budget => return Ok(None),
            Some(Known::Over(_)) | None => {}
        }
        self.known.insert(link, Known::Open);
        let parent = self.places[link].parent;
        let taken = self.resolve(tree, parent, text, budget);
        match &taken {
            Ok(Some((end, count))) => self.known.insert(link, Known::Ended(end.clone(), *count)),
            Ok(None) => self.known.insert(link, Known::Over(budget)),
            Err(_) => self.known.remove(&link),
        };
        taken
    }

    /// Follows `text` from the directory at `at` within `budget` texts, this
    /// one included. A text that cannot be read is `None`.
    fn resolve<T: Tree<Directory = D>>(
        &mut self,
        tree: &mut T,
        mut at: Node,
        text: Option<&[u8]>,
        budget: usize,
    ) -> Result<Taken, GitError> {
        if budget == 0 {
            return Ok(None);
        }
        let mut hops = 1;
        let text = match text.map(std::str::from_utf8) {
            None => return Ok(Some(refusal(Code::RepoUnreadable, hops))),
            Some(Ok(text)) if !text.is_empty() => text,
            Some(_) => return Ok(Some(refusal(Code::SymlinkDangling, hops))),
        };
        if text.starts_with('/') {
            return Ok(Some(refusal(Code::SymlinkOutside, hops)));
        }
        let mut names = text.split('/').peekable();
        while let Some(name) = names.next() {
            let last = names.peek().is_none();
            match name {
                "" | "." => continue,
                ".." if at == ROOT => return Ok(Some(refusal(Code::SymlinkOutside, hops))),
                ".." => {
                    at = self.places[at].parent;
                    continue;
                }
                _ => {}
            }
            let place = self.step(tree, at, name.as_bytes())?;
            let (end, taken) = match &self.places[place].what {
                What::Directory(..) => {
                    at = place;
                    continue;
                }
                What::File(blob, size) if last => {
                    let (blob, size) = (*blob, *size);
                    return Ok(Some((
                        End::File {
                            at: place,
                            blob,
                            size,
                        },
                        hops,
                    )));
                }
                What::Link(text) => {
                    match self.chained(tree, place, text.clone().as_deref(), budget - hops)? {
                        Some(taken) => taken,
                        None => return Ok(None),
                    }
                }
                What::Unreadable => return Ok(Some(refusal(Code::RepoUnreadable, hops))),
                What::File(..) | What::Long | What::Nothing => {
                    return Ok(Some(refusal(Code::SymlinkDangling, hops)));
                }
            };
            hops += taken;
            match end {
                End::Directory(reached) => at = reached,
                End::File { .. } if !last => return Ok(Some(refusal(Code::SymlinkDangling, hops))),
                end => return Ok(Some((end, hops))),
            }
        }
        Ok(Some((End::Directory(at), hops)))
    }
}

/// A refusal by `code` after `hops` texts.
fn refusal(code: Code, hops: usize) -> (End, usize) {
    (End::Refused(code), hops)
}

/// The longest part of a symlink text a message shows, in characters.
const SHOWN_CHARS: usize = 100;

/// `text` as a message or a report shows it: whole when it is at most
/// [`SHOWN_CHARS`] characters, else its start and its length, and only its
/// length when it is `None`, longer than [`MAX_TEXT_BYTES`] and not loaded,
/// so that a long text cannot swell what Board prints. A name that is not
/// UTF-8 is shown lossily, and a control character, a Unicode format
/// character, a line or paragraph separator or a backslash escaped, so that
/// a text cannot forge a line, a terminal escape or a reordering of the
/// message around it.
pub(crate) fn shown(text: Option<&[u8]>) -> String {
    let Some(text) = text else {
        return format!("… (over {MAX_TEXT_BYTES} bytes)");
    };
    let lossy = String::from_utf8_lossy(text);
    let mut chars = lossy.chars();
    let mut shown = String::new();
    for char in chars.by_ref().take(SHOWN_CHARS) {
        let format = matches!(
            get_general_category(char),
            GeneralCategory::Format
                | GeneralCategory::LineSeparator
                | GeneralCategory::ParagraphSeparator
        );
        if char.is_control() || format || char == '\\' {
            shown.extend(char.escape_default());
        } else {
            shown.push(char);
        }
    }
    if chars.next().is_none() {
        return shown;
    }
    format!("{shown}… ({} bytes)", text.len())
}

/// The place of the symlink `entry` in `directory`: its text, or `None`
/// when it cannot be read. A text longer than [`MAX_TEXT_BYTES`] dangles
/// whatever it holds, so one that the entry's size says is longer is not
/// loaded. Every tree gives a symlink's size: a commit's and the index's
/// from its blob, the working tree's from its metadata.
fn text<T: Tree>(
    tree: &mut T,
    directory: &T::Directory,
    entry: &RawEntry<T::Directory>,
) -> What<T::Directory> {
    if entry.size.is_some_and(|size| size > MAX_TEXT_BYTES as u64) {
        return What::Long;
    }
    match tree.read(directory, entry) {
        Ok(Some(text)) => What::Link(Some(text)),
        Ok(None) => What::Long,
        Err(_) => What::Link(None),
    }
}

/// A symlink that leads to no file: why, as `code` says, and its text, cut
/// when long and with control characters escaped. It displays as a noun
/// phrase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub code: Code,
    pub target: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { code, target } = self;
        let which = match code {
            Code::SymlinkOutside => "is absolute or leaves the repository".to_owned(),
            Code::SymlinkLoop => {
                format!("loops or passes through more than {MAX_SYMLINKS} symlinks")
            }
            Code::RepoUnreadable => "leads to a file that cannot be read".to_owned(),
            _ => "names nothing".to_owned(),
        };
        write!(formatter, "a symlink to \"{target}\", which {which}")
    }
}
