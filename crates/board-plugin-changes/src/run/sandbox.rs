//! `changes.run --sandbox`: a dry run of a flow in a throwaway copy of the
//! board and the change's repository.
//!
//! `run` makes a fresh temporary directory, `board-sandbox.<random>` under
//! `TMPDIR`, and copies into it the board's files that git tracks or would
//! track, uncommitted edits included, as a new repository with one commit
//! and no remote. Nested repositories and ignored files are left out. For a
//! change on a repository it fetches every ref of the clone the mount
//! reads into a new repository with no remote, and points the copy's mount
//! record at it; a change on `/` has none. Every other mount record, each
//! `board/mount/v1` with a `repo`, is removed from the copy, so the agent's
//! board reads and writes no other repository, and a write-through put
//! cannot commit into a real clone. Links stay. Both copies commit as the
//! board does, with its git identity in their own config. `run` starts the change
//! in the copy, its task added to the copy's own tasks when none is named,
//! and runs the agent there with `HOME` and the XDG directories inside the
//! temporary directory, whose known boards file lists only the copy.
//!
//! That keeps the agent's `board` calls, its git commits in its worktree
//! and any program that reads user-level configuration, such as Claude
//! Code's settings, off the real board, the real repositories and the
//! person's home. It is not an operating-system sandbox: the agent runs as
//! the person, with the person's permissions, and can read and write any
//! path it names, the real board and repositories included. It
//! authenticates with the variables run passes on, as its home holds no
//! credentials. Nothing removes the temporary directory.
//!
//! The run's record and its stop record are copied back under the change
//! that asked, `--for` or the change whose directory holds `--flow`, at the
//! same stamps; the run record names the throwaway change in `change`, and
//! the copied stop drops its question, which only the copy holds. The run
//! record is attached to that change. The temporary directory is left for
//! inspection, and is never cleaned up.

use std::path::{Path, PathBuf};
use std::time::Duration;

use board_model::Source;

use board_ops::{Elsewhere, Host, PluginError, Run, literal};
use serde_json::{Map, Value, json};

use super::{
    Input, Output, Told, agent_argv, asked_stop, brief_for, change_of, directory_for, recorded,
    set_stop, spawn, which_change,
};
use crate::board::{self, body, rows};
use crate::change::{name_in, read};
use crate::records::BOARD_ITSELF;
use crate::{refusal, to_value};

/// The variable that names the directory a sandbox is made in.
pub(crate) const TEMPORARY: &str = "TMPDIR";
/// How long making the sandbox may take.
const SETUP_DEADLINE: Duration = Duration::from_secs(300);

/// Makes `$1/board` a copy of the board at `$2` as a new repository, and,
/// with `$3`, `$1/repo` a repository holding every ref of the clone at
/// `$3`, checked out at the commit `$4`, and the copy's mount record at
/// `$5` holding `$6`. It removes every other mount record from the copy,
/// each `board/mount/v1` file with a `repo`; links, which have a `path`
/// instead, stay. `$7` is the known boards file of the home at `$1/home`.
const SETUP: &str = r#"set -eu
to=$1 board=$2 clone=$3 commit=$4 file=$5 mount=$6 known=$7
name=$(git -C "$board" config user.name || echo Board)
email=$(git -C "$board" config user.email || echo board@localhost)
mkdir -p "$to/board" "$to/home/.config/board" "$to/home/.cache" "$to/home/.local/share"
printf '%s\n' "$known" > "$to/home/.config/board/config.json"
cd "$board"
git ls-files -z --cached --others --exclude-standard |
  xargs -0 sh -c 'for f do case $f in */) ;; *) if [ -e "$f" ] || [ -L "$f" ]; then printf "%s\0" "$f"; fi;; esac; done' sh |
  tar --null -T - -cf - | tar -xf - -C "$to/board"
cd "$to/board"
git init -q
git config user.name "$name"
git config user.email "$email"
if [ -n "$clone" ]; then
  if [ ! -f "$file" ]; then
    echo "the mount record $file is not in the board's own tree" >&2
    exit 3
  fi
  printf '%s\n' "$mount" > "$file"
fi
find . -type f -name '*.json' ! -path './.git/*' \
  -exec grep -lE '"\$type"[[:space:]]*:[[:space:]]*"board/mount/v1"' {} + |
  while IFS= read -r each; do
    each=${each#./}
    if [ "$each" != "$file" ] && grep -qE '"repo"[[:space:]]*:' "$each"; then rm -f "$each"; fi
  done
git add -A
git commit -qm 'A sandbox copy of the board'
if [ -n "$clone" ]; then
  git init -q "$to/repo"
  cd "$to/repo"
  git config user.name "$name"
  git config user.email "$email"
  git fetch -q --update-head-ok "$clone" '+refs/*:refs/*'
  git checkout -q -f --detach "$commit"
fi
"#;

/// A sandbox: its temporary directory, the board copy in it, and the
/// variables that give the agent a home inside it.
struct Sandbox {
    root: PathBuf,
    board: PathBuf,
    home: [(&'static str, String); 4],
}

/// `changes::sandbox::setup`, saying `why`.
fn setup_failed(mount: &str, why: &str) -> PluginError {
    refusal(
        "changes::sandbox::setup",
        format!(
            "the sandbox for a change on {mount} cannot be made: {why}; fix the board's or the \
             clone's git repository, or run without --sandbox"
        ),
        [("mount", json!(mount))],
    )
}

/// Runs `/bin/sh -c script` with `arguments`, and returns what it printed,
/// or why it failed.
fn shell(host: &dyn Host, script: &str, arguments: &[String]) -> Result<String, String> {
    let mut given = vec!["-c".to_owned(), script.to_owned(), "sh".to_owned()];
    given.extend_from_slice(arguments);
    let run = Run {
        program: "/bin/sh",
        arguments: &given,
        directory: None,
        input: b"",
        deadline: SETUP_DEADLINE,
        added: &[],
    };
    match host.run(&run) {
        Ok(exited) if exited.code == Some(0) => {
            Ok(String::from_utf8_lossy(&exited.stdout).trim().to_owned())
        }
        Ok(exited) => Err(String::from_utf8_lossy(&exited.stderr).trim().to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Makes the sandbox for a change on `mount`.
fn make(host: &dyn Host, mount: &str) -> Result<Sandbox, PluginError> {
    let failed = |why: String| setup_failed(mount, &why);
    let board_root = board::resolve(host, BOARD_ITSELF)?
        .map(|answer| answer.location.board.repository)
        .ok_or_else(|| failed("the board's root cannot be read".to_owned()))?;
    let root = shell(
        host,
        r#"mktemp -d "${TMPDIR:-/tmp}/board-sandbox.XXXXXX""#,
        &[],
    )
    .map_err(failed)?;
    let root = PathBuf::from(root);
    let board = root.join("board");
    let known = json!({"boards": [board], "personal": board}).to_string();
    let mut arguments = vec![
        root.display().to_string(),
        board_root.display().to_string(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        known,
    ];
    if mount != BOARD_ITSELF {
        let answer =
            board::resolve(host, mount)?.ok_or_else(|| failed(format!("nothing is at {mount}")))?;
        let location = answer.location;
        let pointer = (location.pointers.last())
            .filter(|_| location.pointers.len() == 1)
            .filter(|pointer| pointer.board.repository == board_root)
            .ok_or_else(|| failed(format!("{mount} is not a mount of the board's own tree")))?;
        let follow = pointer.follow.clone().unwrap_or_else(|| "HEAD".to_owned());
        let commit = match &location.board.source {
            Source::Commit(commit) => commit.to_string(),
            Source::WorkingTree => follow.clone(),
        };
        let record = json!({
            "$type": "board/mount/v1",
            "commit": commit,
            "follow": follow,
            "repo": root.join("repo"),
        });
        arguments[2] = location.board.repository.display().to_string();
        arguments[3] = commit;
        arguments[4].clone_from(&pointer.path);
        arguments[5] = record.to_string();
    }
    shell(host, SETUP, &arguments).map_err(failed)?;
    let home = root.join("home");
    let under = |part: &str| home.join(part).display().to_string();
    Ok(Sandbox {
        home: [
            ("HOME", home.display().to_string()),
            ("XDG_CONFIG_HOME", under(".config")),
            ("XDG_CACHE_HOME", under(".cache")),
            ("XDG_DATA_HOME", under(".local/share")),
        ],
        root,
        board,
    })
}

/// The change a sandboxed run is a dry run for: `--for`, else the change
/// whose directory holds the flow `--flow` names by path, else
/// `changes::sandbox::no_change`.
fn asker(given: &Input) -> Result<String, PluginError> {
    if let Some(asker) = &given.asker {
        return Ok(asker.clone());
    }
    let holder = (given.flow.as_deref())
        .and_then(|flow| flow.strip_prefix("/changes/"))
        .and_then(|rest| rest.split_once('/'))
        .map(|(name, _)| format!("/changes/{name}"));
    holder.ok_or_else(|| {
        refusal(
            "changes::sandbox::no_change",
            "a sandboxed run copies its records back under the change that asked for it, and \
             none is named; give --for /changes/<name>, or --flow as the path of a draft under \
             that change"
                .to_owned(),
            [("flow", json!(given.flow))],
        )
    })
}

/// Refuses a sandboxed run that cannot start a fresh change: one that
/// returns its brief instead of running an agent, or resumes a change.
fn refuse_unrunnable(given: &Input) -> Result<(), PluginError> {
    if given.inline {
        return Err(refusal(
            "changes::sandbox::inline",
            "a sandboxed run starts an agent in the sandbox, and --inline true starts none; leave \
             out --inline, or --sandbox to follow the brief yourself"
                .to_owned(),
            [],
        ));
    }
    if let Some(path) = &given.path {
        return Err(refusal(
            "changes::sandbox::resume",
            format!(
                "a sandboxed run starts a fresh change in a throwaway board, so it cannot resume \
                 {path}; give --mount and --title, or leave out --sandbox"
            ),
            [("path", json!(path))],
        ));
    }
    if given.mount.is_none() || given.title.is_none() {
        return Err(which_change(given));
    }
    Ok(())
}

/// The record at `path` as the board `host` reads it.
fn record_at(host: &dyn Host, path: &str) -> Result<Map<String, Value>, PluginError> {
    let sql = format!("SELECT body FROM records WHERE logical = {}", literal(path));
    Ok(body(rows(host, &sql)?.first().and_then(|row| row.first())))
}

/// Copies the run record at `run` and the stop record at `stopped`, of the
/// change at `path` in the sandbox `inner`, under `asker` in `host`, and
/// attaches the run to it; returns the copies' paths.
fn copy_back(
    host: &dyn Host,
    inner: &dyn Host,
    asker: &str,
    path: &str,
    (run, stopped): (&str, &str),
) -> Result<(String, String), PluginError> {
    let mut record = record_at(inner, run)?;
    let mut stop = record_at(inner, stopped)?;
    let to_run = format!("{asker}/runs/{}", name_in(run));
    let to_stop = format!("{asker}/stops/{}", name_in(stopped));
    record.insert("stopped".to_owned(), json!(to_stop));
    record.insert("change".to_owned(), json!(name_in(path)));
    stop.remove("question");
    board::put(
        host,
        &[
            json!({"path": to_stop, "expect": "none", "record": stop}),
            json!({"path": to_run, "expect": "none", "record": record}),
        ],
    )?;
    crate::phases::attach(host, &json!({"path": asker, "ref": to_run}))?;
    Ok((to_run, to_stop))
}

pub(super) fn run(host: &dyn Host, given: &Input) -> Result<Value, PluginError> {
    refuse_unrunnable(given)?;
    let asker = asker(given)?;
    read(host, &asker)?;
    let argv = agent_argv(host)?;
    let wanted = asked_stop(given)?;
    let mount = given.mount.as_deref().unwrap_or(BOARD_ITSELF);
    let mount = match mount.trim_end_matches('/') {
        "" => BOARD_ITSELF,
        trimmed => trimmed,
    };
    let made = make(host, mount)?;
    let inner = (host.board(&Elsewhere::Directory(made.board.clone())))
        .map_err(|refused| board::refused("-C", &refused))?;
    let inner = inner.as_ref();
    let path = change_of(inner, given, Some(BOARD_ITSELF))?;
    let (flow, stop) = set_stop(inner, &path, wanted)?;
    let (brief, shown, mount) = brief_for(inner, &path, &flow, &stop, given.budget)?;
    let mut output = Output::of(&path, shown);
    output.sandbox = Some(made.root.display().to_string());
    if output.stop.reached {
        return Ok(to_value(output));
    }
    let directory: PathBuf = directory_for(inner, &path, &mount)?;
    let extra: Vec<(&str, &str)> = (made.home.iter())
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let ran = spawn(host, &path, &argv, &directory, &brief, &extra)?;
    let told = Told {
        flow: &flow.name,
        stop: &stop,
        argv: &argv,
        budget: given.budget,
        sandbox: true,
    };
    let written = recorded(inner, &path, &told, &ran)?;
    output.finish(inner, &path, &written, &ran, Path::new(&directory))?;
    let (run, stopped) = copy_back(host, inner, &asker, &path, (&written.run, &written.stopped))?;
    output.run = Some(run);
    output.stopped = Some(stopped);
    Ok(to_value(output))
}
