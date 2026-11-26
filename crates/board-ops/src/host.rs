//! Plugins built into Board, and what Board offers them
//! (docs/plugins/built-in.md).

use std::cell::RefCell;
use std::ffi::OsString;
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use board_model::{
    CapabilityName, Code, Fields, LogicalPath, Moved, Order, PluginName, RecordType, Refusal,
};
use board_plugin::child::{Exited, ProcessError};
use board_plugin::{CallReport, Manifest, Signers, Target};
pub use board_tree::Origin;
use serde_json::Value;

use crate::effects::{
    Asker, DecideReport, OwnerWriteError, Shown, SignOrderError, VerifyOrderError,
};
use crate::known::{self, KnownBoards};
use crate::mount::{MountError, MountUpdateReport};
use crate::plugins::Plugins;
use crate::put::{PutError, PutReport};
use crate::query::{MAX_LIMIT, QueryError, QueryReport};
use crate::read::{GetReport, ReadError};
use crate::refs::{Absent, RefsError, RefsReport};
use crate::sync::{Guard, SyncError, Synced};
use crate::{Board, OpenError};

mod error;
mod memo;
mod settings;
pub use error::PluginError;
pub(crate) use memo::Memo;
pub use settings::{Settings, Variables};

/// A plugin compiled into Board, which runs in Board's own process.
pub trait Plugin: Sync {
    /// Its manifest, which `Manifest::built_in` reads.
    fn manifest(&self) -> Manifest;
    /// The schema it ships for each type its manifest declares.
    fn schemas(&self) -> Vec<(RecordType, Value)>;
    /// Runs `capability` with `input`, which its input schema admitted.
    /// Board checks what it returns as it checks a process plugin's reply.
    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError>;
    /// The prefixes its records moved from, and to, as logical paths, which
    /// a resolution that names nothing follows (docs/plugins/calling/
    /// commands.md, Renamed plugins). Each pair must pass `Moved::new`.
    fn moved(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
    /// Each type it shipped under a former name, and the type that replaces
    /// it. A board may hold the schema of either one of a pair alone: one
    /// that has not taken the new schemas yet holds the old, and one that
    /// has migrated may hold only the new.
    fn renamed_types(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
    /// The fields of its types that a command-line `board put` fills when
    /// the caller leaves them out (docs/architecture/writes.md, Fills and
    /// drops).
    fn fills(&self) -> &'static [crate::Fill] {
        &[]
    }
}

/// What Board offers a built-in plugin while it runs a capability.
pub trait Host {
    /// Runs `sql` as `board query` does, returning at most `limit` rows.
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError>;
    /// Answers `paths` as `board refs` does, each in order, through one
    /// snapshot, reading each reference's target at `fields`, JSON
    /// pointers, as `board refs --field` does.
    fn refs(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
        fields: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError>;
    /// Reads `paths` as `board get` does, each in order, through one
    /// snapshot.
    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError>;
    /// Writes `lines`, each one line of `board put --batch`, as that
    /// command does under `BOARD_PLUGIN`: every line checked, the effects
    /// check run as the plugin, and one commit or nothing.
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError>;
    /// Writes `lines` as [`Host::put`] does, with the trailer
    /// `Board-Bulk: <reason>` that `bulk` gives on its commit. A host that
    /// makes no commit, as a test's does, writes as [`Host::put`] does.
    fn put_bulk(&self, lines: &[Value], bulk: &crate::Bulk) -> Result<PutReport, PutError> {
        let _ = bulk;
        self.put(lines)
    }
    /// Checks `lines` as [`Host::put`] would, against the schema of each
    /// record's type and the rest of its checks, and writes nothing. A host
    /// with no schemas, as a test's, finds nothing wrong.
    fn check(&self, lines: &[Value]) -> Result<(), PutError> {
        let _ = lines;
        Ok(())
    }
    /// Runs `target` with `input` as the plugin's own `board call` does:
    /// one call deeper, under the `BOARD_PLUGIN` it would set, with only
    /// the capability's declared `env` among the variables. Board's
    /// refusal, or the called plugin's, is returned as `board call` prints
    /// it.
    fn call(&self, target: &Target, input: &Value) -> Result<CallReport, Box<Refusal>>;
    /// The board that `board --personal` or `board -C <directory>` opens
    /// when the plugin runs it, as the same plugin at the same depth. It
    /// reads each of its mounts at its pin and may write, even when this
    /// run reads a mount at a checkout, because `--checkout` names a mount
    /// of the board this run opened and no other.
    fn board(&self, elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>>;
    /// Whether the plugin may run `capability`, `<plugin>.<capability>`, at `path`,
    /// showing `shown`, as `board decide` answers under `BOARD_PLUGIN`: the
    /// owner's signed approval admits it, their refusal refuses it, and with
    /// no answer the pending effect is written in the personal board and
    /// it is refused as asked.
    fn decide(
        &self,
        capability: &str,
        path: &LogicalPath,
        shown: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>>;
    /// As [`Host::decide`], but with no answer it writes no pending effect and
    /// returns `None`. The default says no answer, for a host with no owner.
    fn decided(
        &self,
        _capability: &str,
        _path: &LogicalPath,
        _shown: Option<&Shown>,
    ) -> Result<Option<DecideReport>, Box<Refusal>> {
        Ok(None)
    }
    /// Syncs this host's board as `board sync` does, or as `board sync
    /// --no-push` without `push`, within `deadline`: it waits for the board
    /// lock no longer than `board sync` does, and git still fetching or
    /// pushing when the deadline passes is stopped, which is
    /// `board::sync::timeout`. A plugin syncs a notes clone on the host
    /// [`Host::board`] gives for the clone's directory, as `board -C <clone>
    /// sync` does.
    fn sync(&self, push: bool, deadline: Duration) -> Result<Synced, SyncError>;
    /// As [`Host::sync`], but a sync of the same clone already running is
    /// waited for up to `wait` before this one is refused as
    /// `board::sync::busy`. A host with no such lock ignores `wait`.
    fn sync_waiting(
        &self,
        push: bool,
        deadline: Duration,
        _wait: Duration,
    ) -> Result<Synced, SyncError> {
        self.sync(push, deadline)
    }
    /// Syncs this host's board as [`Board::sync_guarded`] does, with
    /// `guard`'s checks. A host with no board of its own, such as a test's,
    /// follows no remote, which is `board::sync::no_remote`.
    fn sync_guarded(
        &self,
        push: bool,
        guard: &mut Guard<'_>,
        deadline: Duration,
    ) -> Result<Synced, SyncError> {
        let _ = (push, guard, deadline);
        Err(board_tree::SyncError::NoRemote { branch: None }.into())
    }
    /// Moves the mount at `path` to the commit its `follow` names, as
    /// `board mount update` does, waiting for the board lock no longer than
    /// `deadline` or that command, whichever is shorter; past it, the update
    /// is `board::put::locked`.
    fn mount_update(
        &self,
        path: &LogicalPath,
        deadline: Duration,
    ) -> Result<MountUpdateReport, MountError>;
    fn settings(&self) -> &Settings;
    /// Says that a walk passed over a path for what `refusal` names. Each
    /// distinct warning is printed once after the call, beside its output
    /// or its refusal.
    /// A host with nowhere to print it drops it.
    fn warn(&self, refusal: &Refusal) {
        let _ = refusal;
    }
    /// The checkout of a mounted repository the run started in, when Board
    /// found the board through that mount and reads it at its pin, as
    /// [`Board::started_in`] gives it. A host with no such checkout, such
    /// as one [`Host::board`] opened, has none.
    fn started_in(&self) -> Option<crate::StartedIn> {
        None
    }
    /// The `origin` of the git repository holding the caller's directory,
    /// as `git remote get-url origin` there gives it, read from git's
    /// config so no process starts. Without a caller, or outside a
    /// repository, it is [`Origin::Outside`]; a repository that gives no
    /// origin is [`Origin::Unread`], saying why.
    fn caller_remote(&self) -> Origin {
        let Some(directory) = &self.settings().caller else {
            return Origin::Outside;
        };
        match board_tree::origin(directory) {
            Origin::Unread(why) => Origin::Unread(format!(
                "git gave no origin for {}: {why}",
                directory.display()
            )),
            read => read,
        }
    }
    /// Whether the caller's directory is in the called board's own git
    /// working tree, not a repository nested in it. By default it is not.
    fn caller_in_board(&self) -> bool {
        false
    }
    /// The `origin` of the git repository at `clone` itself, as `git remote
    /// get-url origin` gives it there with `GIT_CEILING_DIRECTORIES` set to
    /// the clone's parent, read from git's config so no process starts. A
    /// directory that is not a repository is [`Origin::Outside`], even inside
    /// another repository; a repository that gives no origin is
    /// [`Origin::Unread`], saying why.
    fn clone_origin(&self, clone: &Path) -> Origin {
        board_tree::own_origin(clone)
    }
    /// Runs `run` through board-plugin's process module with the settings'
    /// `environment` and nothing else, as the leader of its own process
    /// group. Every process still in the group is killed when it exits, past
    /// its deadline or when Board is stopped. It is [`Host::run_hiding`]
    /// hiding nothing, so a host that runs processes its own way, such as a
    /// test's, overrides `run_hiding` alone.
    fn run(&self, run: &Run<'_>) -> Result<Exited, ProcessError> {
        self.run_hiding(run, &[])
    }
    /// [`Host::run`] without the variables `hidden` names, such as model
    /// credentials that only a nested call needs. The settings' other
    /// variables are kept.
    fn run_hiding(&self, run: &Run<'_>, hidden: &[&str]) -> Result<Exited, ProcessError> {
        self.settings().run_hidden(run, hidden)
    }
    /// The logical path of `directory` at each mount of this board whose
    /// repository holds it, or else whose repository has its repository's
    /// origin, as `board refs <directory>` would name it, or
    /// `board::path::malformed` when one of them is too long to be a logical
    /// path. A host with no board of its own, such as a test's, finds none.
    fn places(&self, _directory: &Path) -> Result<Vec<LogicalPath>, Box<Refusal>> {
        Ok(Vec::new())
    }
    /// The value cached under `digest` in `place`, a directory under the
    /// cache home such as `board-rules/judged`, or none when there is
    /// none, it cannot be read, or the cache home is unknown.
    fn cached(&self, place: &str, digest: &[u8]) -> Option<Vec<u8>> {
        board_index::cached(&cache_place(self.settings(), place)?, digest)
    }
    /// Caches `value` under `digest` in `place`. Deleting the cache loses
    /// nothing but the time to compute the value again. With no cache home,
    /// or a `place` that is not plain relative names, nothing is cached.
    fn cache(&self, place: &str, digest: &[u8], value: &[u8]) -> std::io::Result<()> {
        match cache_place(self.settings(), place) {
            Some(directory) => board_index::cache(&directory, digest, value),
            None => Ok(()),
        }
    }
    /// Shows the owner `order`'s statement through the signer, with 10
    /// minutes to read it and touch the sensor, and returns their armored
    /// signature once it verifies as `verify_order` checks it. Without one
    /// it says why: the owner asked for changes, with their words, or
    /// cancelled; there is no signer; or the signer failed. Only Board's own
    /// host signs; any other refuses with [`SignOrderError::NotOffered`].
    fn sign_order(&self, _order: &Order) -> Result<String, SignOrderError> {
        Err(SignOrderError::NotOffered)
    }
    /// `owner`, when Board's allowed signers file, or else the attention
    /// file with an `orders::signers::legacy` warning, admits the key whose
    /// `signature` of `order`'s statement verifies, or why not. Only Board's
    /// own host verifies; any other refuses with `NotOffered`.
    fn verify_order(&self, _order: &Order, _sig: &str) -> Result<&'static str, VerifyOrderError> {
        Err(VerifyOrderError::NotOffered)
    }
    /// Board's allowed signers file with the attention file's keys merged in
    /// for orders, read and never written; only Board's own host reads them.
    fn order_signers(&self) -> Result<Signers, VerifyOrderError> {
        Err(VerifyOrderError::NotOffered)
    }
    /// Each board the known boards file lists. A host with no known boards
    /// file, such as a test's, lists none.
    fn known_boards(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    /// Shows the owner the statement of their put of `record` at `path` in
    /// the personal board through the signer, and returns their armored
    /// signature once it verifies against the allowed signers file. A
    /// record a plugin trusts only with this signature carries it beside
    /// these fields. Only Board's own host signs; any other refuses with
    /// [`OwnerWriteError::NotOffered`].
    fn sign_owner_write(&self, _path: &str, _record: &Value) -> Result<String, OwnerWriteError> {
        Err(OwnerWriteError::NotOffered)
    }
    /// Checks that `signature` is the owner's signature of their put of
    /// `record` at `path` in the personal board. Only Board's own host
    /// verifies; any other refuses with [`OwnerWriteError::NotOffered`].
    fn verify_owner_write(
        &self,
        _path: &str,
        _record: &Value,
        _signature: &str,
    ) -> Result<(), OwnerWriteError> {
        Err(OwnerWriteError::NotOffered)
    }
}

/// `place` under the settings' cache home, when there is one and `place`
/// is relative names only, none of them `.` or `..`.
fn cache_place(settings: &Settings, place: &str) -> Option<PathBuf> {
    let plain = !place.is_empty()
        && (place.split('/')).all(|name| !name.is_empty() && name != "." && name != "..");
    plain.then(|| settings.cache_home.as_ref().map(|home| home.join(place)))?
}

/// A process a built-in plugin runs through [`Host::run`]: its program,
/// found on the environment's `PATH`, its arguments, the directory it runs
/// in, what it reads on stdin, and how long it may run.
#[derive(Clone, Copy, Debug)]
pub struct Run<'a> {
    pub program: &'a str,
    pub arguments: &'a [String],
    /// The directory it runs in; none runs it in a new empty temporary
    /// directory, removed when it exits.
    pub directory: Option<&'a Path>,
    pub input: &'a [u8],
    pub deadline: Duration,
    /// Variables set for this process alone, over the settings' variables
    /// of the same names, such as a result a hook the process runs reads.
    pub added: &'a [(&'a str, &'a str)],
}

/// Another board a built-in plugin acts on, as its own `board` opens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Elsewhere {
    /// `--personal`: the personal board the known boards file names.
    Personal,
    /// `-C <directory>`: the board holding the directory, read against
    /// the called board's root, or the one known board that mounts it.
    Directory(PathBuf),
}

/// How many rows a query returns: 1 to `MAX_LIMIT`, or all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit(usize);

impl Limit {
    pub const ALL: Self = Self(usize::MAX);

    /// At most `rows` rows; `None` outside 1 to `MAX_LIMIT`.
    pub fn rows(rows: usize) -> Option<Self> {
        (1..=MAX_LIMIT).contains(&rows).then_some(Self(rows))
    }
}

/// SQL `text` as a string literal, for the statement a built-in plugin gives
/// [`Host::query`], which takes no parameters.
pub fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The plugins built into a Board.
#[derive(Clone, Copy, Default)]
pub struct BuiltIn(pub &'static [&'static dyn Plugin]);

impl fmt::Debug for BuiltIn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.0.iter().map(|plugin| plugin.manifest().name))
            .finish()
    }
}

/// The host a built-in plugin sees during one call.
pub(crate) struct Hosted<'a> {
    pub(crate) board: HostBoard<'a>,
    pub(crate) settings: Settings,
    /// The plugin a put is checked as, or the `BOARD_PLUGIN` that names
    /// none.
    pub(crate) plugin: Result<PluginName, OsString>,
    /// What the plugin's own `board` reads: the caller's plugins directory,
    /// cache, known boards and signing, and the environment
    /// `Environment::nested` gives it.
    pub(crate) plugins: Plugins<'a>,
    /// Where a process plugin's `board` runs: the called board's root.
    pub(crate) root: &'a Path,
    /// What [`Host::warn`] was told, one line each, without repeats. A host
    /// [`Host::board`] opens adds to the same list.
    pub(crate) warnings: &'a RefCell<Vec<String>>,
    /// The snapshots this call's reads share, with the call that called it
    /// when this call's capability is read-only, and with the host that
    /// opened this one on the same board.
    pub(crate) memo: Rc<Memo>,
}

impl Hosted<'_> {
    fn add_warning(&self, line: String) {
        let mut warnings = self.warnings.borrow_mut();
        if !warnings.contains(&line) {
            warnings.push(line);
        }
    }
}

/// The board a host acts on: the called one, or one it opened.
pub(crate) enum HostBoard<'a> {
    Called(&'a Board),
    Opened(Box<Board>),
}

impl Deref for HostBoard<'_> {
    type Target = Board;

    fn deref(&self) -> &Board {
        match self {
            Self::Called(board) => board,
            Self::Opened(board) => board,
        }
    }
}

impl Hosted<'_> {
    /// The board opened again, with the same built-in plugins and build,
    /// and the same mount read at a checkout, as the plugin's own `board`
    /// opens it. Its timings stay in the board it opens, outside the call's
    /// trace.
    fn again(&self) -> Result<Board, OpenError> {
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
    fn put_marked(
        &self,
        lines: &[Value],
        bulk: Option<&crate::Bulk>,
    ) -> Result<PutReport, PutError> {
        let (board, text) = self.batch_of(lines, bulk)?;
        board.put_batch(&mut text.as_bytes())
    }

    /// The board opened as the plugin, and `lines` as the text of a batch.
    fn batch_of(
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

    fn plugin(&self) -> Result<PluginName, PutError> {
        (self.plugin.clone()).map_err(|text| PutError::BadPlugin { text })
    }
}

/// The logical path of each of `places`, or the refusal of the first whose
/// path is too long to be one, so a caller never mistakes a dropped place
/// for no place.
fn prefixes(places: &[board_tree::Place]) -> Result<Vec<LogicalPath>, Box<Refusal>> {
    (places.iter())
        .map(|place| {
            (place.prefix())
                .map_err(|error| Box::new(Refusal::new(error.code(), &error, &Fields::default())))
        })
        .collect()
}

fn opened(error: &OpenError) -> Refusal {
    Refusal::new(error.code(), error, &error.fields())
}

impl Hosted<'_> {
    /// Opens the board again asking as the plugin, with the personal
    /// board and signing `board decide` reads.
    fn asking(&self, capability: &str) -> Result<Board, Box<Refusal>> {
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

impl Host for Hosted<'_> {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        (self.memo).query(&self.board, self.plugins.cache, sql, limit.0)
    }

    fn refs(
        &self,
        paths: &[LogicalPath],
        absent: Absent,
        fields: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        (self.memo).refs(&self.board, self.plugins.cache, paths, absent, fields)
    }

    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        Ok(self.board.get_each(paths)?.collect())
    }

    fn places(&self, directory: &Path) -> Result<Vec<LogicalPath>, Box<Refusal>> {
        let places = (self.board.places(directory, self.plugins.cache))
            .map_err(|error| Box::new(Refusal::new(error.code(), &error, &Fields::default())))?;
        prefixes(&places)
    }

    /// Opens the board again as the plugin, as a put through a mount opens
    /// the mounted board, so every check reads the plugin where it already
    /// does.
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        self.memo.forget();
        self.put_marked(lines, None)
    }

    fn put_bulk(&self, lines: &[Value], bulk: &crate::Bulk) -> Result<PutReport, PutError> {
        self.memo.forget();
        self.put_marked(lines, Some(bulk))
    }

    fn check(&self, lines: &[Value]) -> Result<(), PutError> {
        let (board, text) = self.batch_of(lines, None)?;
        board.check_batch(&mut text.as_bytes())
    }

    /// Shares this call's snapshots with a read-only capability, and
    /// drops them before any other.
    fn call(&self, target: &Target, input: &Value) -> Result<CallReport, Box<Refusal>> {
        let board = self.again().map_err(|error| Box::new(opened(&error)))?;
        // As bytes, so the input passes the size limit and parse a process
        // plugin's `board call` reads it through.
        let called = board.call_sharing(
            target,
            input.to_string().as_bytes(),
            &self.plugins,
            &self.memo,
        );
        // The processes it started, and the snapshots it took, count in
        // this call's trace.
        self.board.trace.borrow_mut().fold(board.take_trace());
        let report = called.map_err(|error| Box::new(error.refusal()))?;
        report
            .warnings
            .iter()
            .for_each(|line| self.add_warning(line.clone()));
        Ok(report)
    }

    fn warn(&self, refusal: &Refusal) {
        let hint = refusal.hint.as_deref().unwrap_or("no next step is known");
        self.add_warning(format!(
            "{}: {}; {hint}",
            refusal.code.as_str(),
            refusal.message
        ));
    }

    /// The canonical working-tree roots above each are the same one.
    fn caller_in_board(&self) -> bool {
        let caller = self
            .settings
            .caller
            .as_deref()
            .and_then(board_tree::repository_root);
        caller.is_some_and(|caller| board_tree::repository_root(self.root) == Some(caller))
    }

    fn board(&self, elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        let home = self.board.repositories().home();
        let known = self.plugins.known.cloned().unwrap_or_default();
        let start = match elsewhere {
            Elsewhere::Personal => known.personal(home)?,
            Elsewhere::Directory(directory) => {
                known::entered(self.root, &[directory.as_os_str().to_owned()])?
            }
        };
        let (mut board, _) = known::open(&start, home, self.plugins.cache, || Ok(known))?;
        board.build_in(self.board.built_in.0);
        board.built_by(self.board.build);
        board.running_as(self.board.running.clone());
        let memo = Memo::opened(&self.memo, &self.board, &board);
        Ok(Box::new(Hosted {
            board: HostBoard::Opened(Box::new(board)),
            settings: self.settings.clone(),
            plugin: self.plugin.clone(),
            plugins: self.plugins.clone(),
            root: self.root,
            warnings: self.warnings,
            memo,
        }))
    }

    fn decide(
        &self,
        capability: &str,
        path: &LogicalPath,
        shown: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        self.memo.forget();
        (self
            .asking(capability)?
            .decide_capability(capability, path, shown))
        .map_err(|error| Box::new(error.refusal()))
    }

    fn decided(
        &self,
        capability: &str,
        path: &LogicalPath,
        shown: Option<&Shown>,
    ) -> Result<Option<DecideReport>, Box<Refusal>> {
        (self
            .asking(capability)?
            .decided_capability(capability, path, shown))
        .map_err(|error| Box::new(error.refusal()))
    }

    fn sync(&self, push: bool, deadline: Duration) -> Result<Synced, SyncError> {
        self.memo.forget();
        self.board.sync(push, deadline)
    }

    fn sync_waiting(
        &self,
        push: bool,
        deadline: Duration,
        wait: Duration,
    ) -> Result<Synced, SyncError> {
        self.memo.forget();
        self.board.sync_waiting(push, deadline, wait)
    }

    fn sync_guarded(
        &self,
        push: bool,
        guard: &mut Guard<'_>,
        deadline: Duration,
    ) -> Result<Synced, SyncError> {
        self.memo.forget();
        self.board.sync_guarded(push, guard, deadline)
    }

    fn mount_update(
        &self,
        path: &LogicalPath,
        deadline: Duration,
    ) -> Result<MountUpdateReport, MountError> {
        self.memo.forget();
        self.board.mount_update(path, deadline)
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }

    fn run_hiding(&self, run: &Run<'_>, hidden: &[&str]) -> Result<Exited, ProcessError> {
        (self.memo).forget_running(&self.board, run.directory);
        self.settings.run_hidden(run, hidden)
    }

    fn started_in(&self) -> Option<crate::StartedIn> {
        match &self.board {
            HostBoard::Called(board) => board.started_in(),
            HostBoard::Opened(_) => None,
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "Hosted::sign_order is the one caller of the effects module's order signer"
    )]
    fn sign_order(&self, order: &Order) -> Result<String, SignOrderError> {
        let root = self.board.repository().path();
        crate::effects::sign_order(&self.plugins.signing, root, order)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "Hosted::verify_order is the one caller of the effects module's order verifier"
    )]
    fn verify_order(
        &self,
        order: &Order,
        signature: &str,
    ) -> Result<&'static str, VerifyOrderError> {
        let signing = &self.plugins.signing;
        let signer = crate::effects::verify_order(signing, order, signature)?;
        crate::effects::legacy(signing, &signer).inspect(|warning| self.warn(warning));
        Ok(signer.principal)
    }

    fn order_signers(&self) -> Result<Signers, VerifyOrderError> {
        crate::effects::order_signers(&self.plugins.signing)
    }

    fn known_boards(&self) -> Vec<PathBuf> {
        self.plugins.known.map_or_default(KnownBoards::boards)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "Hosted::sign_owner_write is the one caller of the effects module's write signer"
    )]
    fn sign_owner_write(&self, path: &str, record: &Value) -> Result<String, OwnerWriteError> {
        let root = self.board.repository().path();
        crate::effects::sign_owner_write(&self.plugins.signing, root, path, record)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "Hosted::verify_owner_write is the one caller of the effects module's write verifier"
    )]
    fn verify_owner_write(
        &self,
        path: &str,
        record: &Value,
        signature: &str,
    ) -> Result<(), OwnerWriteError> {
        crate::effects::verify_owner_write(&self.plugins.signing, path, record, signature)
    }
}

impl Board {
    /// Builds `plugins` into this board's calls, ahead of installed ones.
    /// Its resolutions follow the moved prefixes these plugins'
    /// [`Plugin::moved`] gives. An installed plugin declares none, since its
    /// manifest may not name `moved`.
    ///
    /// # Panics
    ///
    /// When a plugin's moved pair is not two logical paths `Moved::new`
    /// admits. The pairs are constants, so every test that builds the
    /// plugins in fails on such a bug before a binary ships it.
    pub fn build_in(&mut self, plugins: &'static [&'static dyn Plugin]) {
        self.built_in = BuiltIn(plugins);
        let moved = (plugins.iter().flat_map(|plugin| plugin.moved()))
            .map(|(from, to)| {
                let (from, to) = (from.parse().ok(), to.parse().ok());
                (from.zip(to).and_then(|(from, to)| Moved::new(from, to)))
                    .expect("a built-in plugin declares a valid moved prefix")
            })
            .collect();
        self.moving(moved);
    }
}

#[cfg(test)]
mod tests;
