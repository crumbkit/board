//! The crates of a Cargo workspace in a mounted repository: their members,
//! the workspace crates each depends on, and the lines of each crate and
//! of each top-level module, read through the host. A mount that holds no
//! workspace, or a manifest that does not parse, gives no crates and says
//! why, and the page then knows the system from its changes alone.
//!
//! The page's mockup also showed each crate's churn, from `git log`. A
//! plugin reads files and records and has no history to read, so the page
//! drops churn.

use std::collections::{BTreeMap, BTreeSet};

use board_ops::{Host, PluginError, literal};

use crate::host::{get_texts, rows, text};
use crate::model::{CrateFact, ModuleFact};

/// Whether `pattern`, in which `*` stands for any run of characters, matches
/// the whole of `text`.
fn wild(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, tail)) => text.strip_prefix(head).is_some_and(|rest| {
            (0..=rest.len())
                .filter(|at| rest.is_char_boundary(*at))
                .any(|at| wild(tail, &rest[at..]))
        }),
    }
}

/// Whether the directory `dir` is one of `patterns`: each is `/`-separated
/// segments, and a segment may hold `*`.
fn listed(patterns: &[String], dir: &str) -> bool {
    patterns.iter().any(|pattern| {
        let (pattern, dir): (Vec<&str>, Vec<&str>) = (
            pattern.trim_start_matches("./").split('/').collect(),
            dir.split('/').collect(),
        );
        pattern.len() == dir.len()
            && pattern
                .iter()
                .zip(&dir)
                .all(|(pattern, dir)| wild(pattern, dir))
    })
}

/// The lines of `text`: each newline, and a last line with none.
fn lines(text: &str) -> u64 {
    let newlines = text.bytes().filter(|byte| *byte == b'\n').count();
    let last = u64::from(!text.is_empty() && !text.ends_with('\n'));
    u64::try_from(newlines).expect("a count of lines fits in u64") + last
}

/// The names of the public functions, structs, enums and traits that start
/// a line of `text`, as `pub fn name`, `pub(crate) struct name` or
/// `pub async fn name` do.
fn items(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("pub")?;
            let rest = rest
                .strip_prefix("(crate)")
                .unwrap_or(rest)
                .strip_prefix(' ')?;
            let rest = rest.strip_prefix("async ").unwrap_or(rest);
            let (kind, name) = rest.split_once(' ')?;
            let name: String = name
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (["fn", "struct", "enum", "trait"].contains(&kind) && !name.is_empty()).then_some(name)
        })
        .collect()
}

/// A list of strings at `key` of `table`.
fn strings(table: &toml::Table, key: &[&str]) -> Vec<String> {
    let mut at = Some(table);
    for name in &key[..key.len() - 1] {
        at = at
            .and_then(|table| table.get(*name))
            .and_then(toml::Value::as_table);
    }
    (at.and_then(|table| table.get(key[key.len() - 1]))
        .and_then(toml::Value::as_array))
    .into_iter()
    .flatten()
    .filter_map(|value| value.as_str().map(str::to_owned))
    .collect()
}

fn parse(path: &str, text: &str) -> Result<toml::Table, String> {
    text.parse()
        .map_err(|error| format!("{path} does not parse: {error}"))
}

/// The names that a manifest's dependency tables name, by the package each
/// refers to.
fn dependencies(manifest: &toml::Table) -> BTreeSet<String> {
    ["dependencies", "dev-dependencies", "build-dependencies"]
        .into_iter()
        .filter_map(|section| manifest.get(section)?.as_table())
        .flat_map(|table| {
            table.iter().map(|(name, value)| {
                (value
                    .as_table()
                    .and_then(|table| table.get("package")?.as_str()))
                .unwrap_or(name)
                .to_owned()
            })
        })
        .collect()
}

/// The twelve longest names, longest first.
fn top_items(names: BTreeSet<String>) -> Vec<String> {
    let mut names: Vec<String> = names.into_iter().collect();
    names.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    names.truncate(12);
    names
}

/// The crates of the workspace whose files are `files`, by path from the
/// mount's root: every `Cargo.toml` and every `.rs` file. The error says why
/// there are none.
pub(crate) fn crates(files: &BTreeMap<String, String>) -> Result<Vec<CrateFact>, String> {
    let root = files
        .get("Cargo.toml")
        .ok_or("the mount has no Cargo.toml at its root")?;
    let root = parse("Cargo.toml", root)?;
    if !root.get("workspace").is_some_and(toml::Value::is_table) {
        return Err("its Cargo.toml has no [workspace]".to_owned());
    }
    let (members, excluded) = (
        strings(&root, &["workspace", "members"]),
        strings(&root, &["workspace", "exclude"]),
    );
    let mut dirs: Vec<String> = (files.keys())
        .filter_map(|path| path.strip_suffix("/Cargo.toml"))
        .filter(|dir| listed(&members, dir) && !listed(&excluded, dir))
        .map(str::to_owned)
        .collect();
    if root.contains_key("package") {
        dirs.push(String::new());
    }
    let mut manifests = Vec::new();
    for dir in &dirs {
        let path = if dir.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{dir}/Cargo.toml")
        };
        let manifest = parse(&path, &files[&path])?;
        if let Some(name) = manifest
            .get("package")
            .and_then(|package| package.get("name")?.as_str())
        {
            manifests.push((dir.clone(), name.to_owned(), dependencies(&manifest)));
        }
    }
    let names: BTreeSet<&str> = manifests.iter().map(|(_, name, _)| name.as_str()).collect();
    // A root package owns only the directories Cargo gives a package.
    let rooted = |path: &str| {
        ["src/", "tests/", "benches/", "examples/"]
            .iter()
            .any(|dir| path.starts_with(dir))
            || path == "build.rs"
    };
    let owner = |path: &str| {
        (manifests.iter())
            .filter(|(dir, ..)| {
                if dir.is_empty() {
                    rooted(path)
                } else {
                    path.starts_with(&format!("{dir}/"))
                }
            })
            .max_by_key(|(dir, ..)| dir.len())
            .map(|(dir, ..)| dir.clone())
    };
    let mut found = Vec::new();
    for (dir, name, deps) in &manifests {
        let mut total = 0;
        let mut modules: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();
        for (path, text) in files.iter().filter(|(path, _)| {
            std::path::Path::new(path)
                .extension()
                .is_some_and(|ext| ext == "rs")
        }) {
            if owner(path).as_ref() != Some(dir) {
                continue;
            }
            let count = lines(text);
            total += count;
            let inside = if dir.is_empty() {
                path.as_str()
            } else {
                &path[dir.len() + 1..]
            };
            if let Some(under) = inside.strip_prefix("src/") {
                let module = under
                    .split_once('/')
                    .map_or_else(|| under.trim_end_matches(".rs"), |(head, _)| head);
                let (lines, names) = modules.entry(module.to_owned()).or_default();
                *lines += count;
                names.extend(items(text));
            }
        }
        found.push(CrateFact {
            id: name.clone(),
            lines: total,
            deps: (deps.iter())
                .filter(|dep| names.contains(dep.as_str()) && *dep != name)
                .cloned()
                .collect(),
            modules: (modules.into_iter())
                .map(|(id, (lines, names))| ModuleFact {
                    id,
                    lines,
                    items: top_items(names),
                })
                .collect(),
        });
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found)
}

/// The crates of the Cargo workspace mounted at `mount`, or the reason
/// there are none.
pub(crate) fn survey(
    host: &dyn Host,
    mount: &str,
) -> Result<Result<Vec<CrateFact>, String>, PluginError> {
    let prefix = format!("{}/", mount.trim_end_matches('/'));
    let sql = format!(
        "SELECT logical FROM entries WHERE kind = 'file' AND substr(logical, 1, {}) = {} \
         AND (logical LIKE '%.rs' OR logical LIKE '%/Cargo.toml') ORDER BY logical",
        prefix.chars().count(),
        literal(&prefix)
    );
    let listed = match rows(host, &sql) {
        Ok(listed) => listed,
        Err(refused) if refused.code == "keeper::board::over_budget" => {
            return Ok(Err(format!(
                "{} holds too many files to list in one board run; survey it by hand",
                prefix.trim_end_matches('/')
            )));
        }
        Err(other) => return Err(other),
    };
    let paths: Vec<String> = listed
        .iter()
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect();
    let files = (get_texts(host, &paths)?.into_iter())
        .map(|(path, text)| (path[prefix.len()..].to_owned(), text))
        .collect();
    Ok(crates(&files))
}

#[cfg(test)]
mod tests;
