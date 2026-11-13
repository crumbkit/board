//! The data layer: runs `board` and `git`, reads the config file, and hands
//! the output to the parsers. It holds no view logic.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{Inputs, WINDOW_SECONDS, ended_paths};
use crate::parse::{
    Config, OpenChange, PhaseEvent, parse_added_log, parse_change_record, parse_changes_open,
    parse_config, parse_map, parse_open_tasks, parse_phase_log, parse_questions,
};
use crate::time::DAY;

/// Where board-top reads from.
#[derive(Debug, Clone)]
pub struct Source {
    /// The board directory, as `board -C` takes it.
    pub board: PathBuf,
    /// The settings.
    pub config: Config,
}

/// Reads the config file. A missing file gives the defaults; an invalid one
/// names the file and what is wrong with it.
pub fn load_config(path: &Path) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config(&text).map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Runs a command and returns its output, or a one-line account of why not:
/// the command, its exit code and the first line of its error.
#[expect(
    clippy::disallowed_methods,
    reason = "this module is the one place board-top starts the board and git commands it reads"
)]
fn run(program: &str, args: &[String]) -> Result<String, String> {
    let shown = format!("{program} {}", args.join(" "));
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("`{shown}` did not start: {error}"))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let error_text = String::from_utf8_lossy(&output.stderr);
    let out_text = String::from_utf8_lossy(&output.stdout);
    let first = [&error_text, &out_text]
        .iter()
        .find_map(|text| text.lines().find(|line| !line.trim().is_empty()))
        .unwrap_or("no error text");
    let code = output
        .status
        .code()
        .map_or("a signal".to_owned(), |code| code.to_string());
    Err(format!("`{shown}` exited with {code}: {first}"))
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// A change path split into the mount prefix and the rest: `/repos/other/changes/x`
/// gives `("/repos/other", "x")`.
fn split_change(path: &str) -> Option<(&str, &str)> {
    let at = path.rfind("/changes/")?;
    Some((&path[..at], &path[at + "/changes/".len()..]))
}

impl Source {
    fn board_args(&self, rest: &[&str]) -> Vec<String> {
        let mut args = vec!["-C".to_owned(), self.board.display().to_string()];
        args.extend(strings(rest));
        args
    }

    fn board(&self, rest: &[&str]) -> Result<String, String> {
        run("board", &self.board_args(rest))
    }

    /// The checkout that holds the records under a mount prefix: the board
    /// itself for no prefix, otherwise the `repo` its mount record names.
    fn directory(&self, prefix: &str) -> Option<PathBuf> {
        if prefix.is_empty() {
            return Some(self.board.clone());
        }
        let file = self
            .board
            .join(format!("{}.json", prefix.trim_start_matches('/')));
        let record: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
        let repo = PathBuf::from(record.get("repo")?.as_str()?);
        (repo.is_absolute() && repo.is_dir()).then_some(repo)
    }

    fn phase_history(
        &self,
        prefixes: &BTreeSet<String>,
        since: i64,
        status: &mut Vec<String>,
    ) -> HashMap<String, Vec<PhaseEvent>> {
        let mut history = HashMap::new();
        let mut missing = Vec::new();
        for prefix in prefixes {
            let Some(directory) = self.directory(prefix) else {
                missing.push(if prefix.is_empty() {
                    "the board".to_owned()
                } else {
                    prefix.clone()
                });
                continue;
            };
            let args = vec![
                "-C".to_owned(),
                directory.display().to_string(),
                "log".to_owned(),
                "--reverse".to_owned(),
                "--no-color".to_owned(),
                "--no-ext-diff".to_owned(),
                format!("--since={since}"),
                "-G\"phase\"".to_owned(),
                "-p".to_owned(),
                "-U0".to_owned(),
                "--format=commit@%aI".to_owned(),
                "--".to_owned(),
                "changes/*.json".to_owned(),
            ];
            match run("git", &args) {
                Ok(text) => {
                    for (name, events) in parse_phase_log(&text) {
                        history.insert(format!("{prefix}/changes/{name}"), events);
                    }
                }
                Err(error) => {
                    status.push(error);
                    missing.push(prefix.clone());
                }
            }
        }
        if !missing.is_empty() {
            status.push(format!(
                "history missing for {}; bars start at the window",
                missing.join(", ")
            ));
        }
        history
    }

    fn task_creation(
        &self,
        tasks: &BTreeSet<String>,
        status: &mut Vec<String>,
    ) -> HashMap<String, i64> {
        let mut by_prefix: HashMap<&str, Vec<&String>> = HashMap::new();
        for task in tasks {
            if let Some(at) = task.rfind("/tasks/") {
                by_prefix.entry(&task[..at]).or_default().push(task);
            }
        }
        let mut created = HashMap::new();
        for (prefix, paths) in by_prefix {
            let Some(directory) = self.directory(prefix) else {
                continue;
            };
            let args = vec![
                "-C".to_owned(),
                directory.display().to_string(),
                "log".to_owned(),
                "--reverse".to_owned(),
                "--diff-filter=A".to_owned(),
                "--format=commit@%aI".to_owned(),
                "--name-only".to_owned(),
                "--".to_owned(),
                "tasks".to_owned(),
            ];
            match run("git", &args) {
                Ok(text) => {
                    let added = parse_added_log(&text);
                    for path in paths {
                        if let Some(at) = added.get(
                            path.strip_prefix(prefix)
                                .unwrap_or(path)
                                .trim_start_matches('/'),
                        ) {
                            created.insert(path.clone(), *at);
                        }
                    }
                }
                Err(error) => status.push(error),
            }
        }
        created
    }

    /// Gathers everything one frame needs. Only the list of open changes is
    /// required; the rest degrades to a note on the status line.
    pub fn load(&self, now: i64) -> Result<Inputs, String> {
        let open = parse_changes_open(&self.board(&["changes", "open", "--json"])?)
            .map_err(|error| format!("`board changes open`: {error}"))?;
        let mut status = Vec::new();

        let (map, questions, open_tasks) = std::thread::scope(|scope| {
            let map = scope.spawn(|| self.board(&["get", "/projects/keeper/map/latest"]));
            let questions =
                scope.spawn(|| self.board(&["plans", "open", "--json", "--limit", "10000"]));
            let tasks = scope
                .spawn(|| self.board(&["tasks", "tree", "--root", "true", "--json", "--full"]));
            (map.join(), questions.join(), tasks.join())
        });
        let joined = |result: std::thread::Result<Result<String, String>>| {
            result.unwrap_or_else(|_| Err("a reader stopped unexpectedly".to_owned()))
        };
        let map = match joined(map) {
            Ok(text) => parse_map(&text)
                .map_err(|error| status.push(format!("work map: {error}")))
                .ok(),
            Err(error) => {
                if !error.contains("nothing is at") {
                    status.push(error);
                }
                None
            }
        };
        let questions = joined(questions)
            .and_then(|text| parse_questions(&text).map_err(|error| error.to_string()))
            .map_err(|error| status.push(format!("questions unavailable: {error}")))
            .ok();
        let open_tasks = joined(open_tasks)
            .and_then(|text| parse_open_tasks(&text).map_err(|error| error.to_string()))
            .map_err(|error| status.push(format!("tasks unavailable: {error}")))
            .ok();

        let mut prefixes: BTreeSet<String> = BTreeSet::from([String::new()]);
        prefixes.extend(
            open.iter()
                .filter_map(|change| split_change(&change.path))
                .map(|(prefix, _)| prefix.to_owned()),
        );
        let since = now - WINDOW_SECONDS - DAY;
        let history = self.phase_history(&prefixes, since, &mut status);

        let mut closed: Vec<OpenChange> = Vec::new();
        for path in ended_paths(&open, &history) {
            match self.board(&["get", &path]).and_then(|text| {
                parse_change_record(&text, &path).map_err(|error| error.to_string())
            }) {
                Ok(change) => closed.push(change),
                Err(error) => status.push(error),
            }
        }
        let tasks: BTreeSet<String> = open
            .iter()
            .chain(&closed)
            .filter_map(|change| change.task.clone())
            .collect();
        let task_created = self.task_creation(&tasks, &mut status);
        Ok(Inputs {
            now,
            limit: self.config.limit,
            open,
            closed,
            history,
            task_created,
            map,
            questions,
            open_tasks,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn splits_a_change_path_at_its_mount() {
        assert_eq!(split_change("/changes/a"), Some(("", "a")));
        assert_eq!(
            split_change("/repos/other/changes/b"),
            Some(("/repos/other", "b"))
        );
        assert_eq!(split_change("/tasks/a.md"), None);
    }

    #[test]
    fn a_missing_config_file_gives_the_default_limit_and_a_bad_one_names_the_file() {
        let missing = Path::new("/nonexistent/board-top/config.toml");
        assert_eq!(load_config(missing).unwrap().limit, 12);

        let mut bad = tempfile::NamedTempFile::new().unwrap();
        bad.write_all(b"limit = [1]").unwrap();
        let error = load_config(bad.path()).unwrap_err();
        assert!(
            error.starts_with(&bad.path().display().to_string()),
            "{error}"
        );

        let mut good = tempfile::NamedTempFile::new().unwrap();
        good.write_all(b"limit = 30").unwrap();
        assert_eq!(load_config(good.path()).unwrap().limit, 30);
    }
}
