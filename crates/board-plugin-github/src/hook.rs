//! The git pre-push hook: the command that installs it in a checkout, the
//! check it runs on what git pushes, and `covers`, which the Claude Code mod
//! asks before a raw write.

use std::path::Path;

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::crossing;
use crate::git::{self, error, git, run, variable};
use crate::parts::ref_parts;
use crate::remote::destinations;
use crate::send::output;

/// The line that marks a hook as this plugin's, so a later install replaces
/// an older copy and no other hook.
const MARKER: &str = "# Installed by `board call github.install-hook`.";
/// The variable that carries the URL git pushes to, kept off board's
/// command line, where `ps` would show it.
pub(crate) const PUSH_URL: &str = "BOARD_GITHUB_PUSH_URL";
/// The hook git runs before each push. It refuses the push when board is
/// missing or the check fails or cannot run. It lets the push go only when
/// the code field of Board's failure document says no config names a
/// personal board. The probe's output holds no content of the push, so no
/// quoted text can match it. The sed reads the first `code` field before
/// any nested object, which is the document's own; JSON escapes a quote
/// inside any string.
const HOOK: &str = r#"#!/bin/sh
# Installed by `board call github.install-hook`.
# Before git pushes, it runs `board --personal call github.pre-push`, which
# has the boundary check what the push sends, and refuses the push when the
# check fails or cannot run.
refs=$(cat)
if ! command -v board >/dev/null 2>&1; then
  echo "board is not on PATH, so the boundary cannot check this push. Next: put board on PATH and push again." >&2
  exit 1
fi
code=$(board --json --personal refs / 2>/dev/null |
  sed -n 's/^{[^{}]*"code"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ "$code" = board::config::not_found ] && exit 0
BOARD_GITHUB_PUSH_URL=$2 board --personal call github.pre-push --remote "$1" --refs "$refs" >/dev/null || exit 1
"#;
/// The line that ends the hook's text in the install command.
const END: &str = "BOARD_PRE_PUSH";
/// The fields of each line git writes on the hook's stdin.
const FIELDS: usize = 4;
/// The config scopes whose `core.hooksPath` serves repositories other than
/// this one.
const SHARED: [&str; 2] = ["global", "system"];

/// Print the command that installs the git pre-push hook in the checkout
/// Board was started in.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "InstallHookInput")]
pub(crate) struct InstallHookInput {}

/// Where the hook goes, whether the same hook is already there, and the
/// shell command that writes it there, its text in a here-document, and
/// makes it executable.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "InstallHookOutput")]
pub(crate) struct InstallHookOutput {
    path: String,
    current: bool,
    command: String,
}

/// The command that installs the pre-push hook in the checkout Board was
/// started in, refusing to replace a different hook. It writes nothing
/// (decision 3 of the plugin porting plan: Board has no path that writes a
/// file outside a board's tree).
pub(crate) fn install_hook(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let InstallHookInput {} = PluginError::read_input("github", "install-hook", input)?;
    let top = git::top(host)?;
    let scoped = run(
        host,
        &top,
        &["config", "--show-scope", "--get", "core.hooksPath"],
        "",
    );
    let scope = scoped.stdout.split('\t').next().unwrap_or_default();
    let named = git(host, &top, &["rev-parse", "--git-path", "hooks"], "")?;
    let hooks = top.join(named.trim());
    if SHARED.contains(&scope) {
        let shown = hooks.display();
        return Err(error(
            "github::hook::shared",
            format!(
                "core.hooksPath in the {scope} git config names {shown}, which serves every \
                 repository, so the hook is not installed; set core.hooksPath for this \
                 repository with `git config core.hooksPath .git/hooks`, then install again"
            ),
            json!({"path": shown.to_string(), "scope": scope}),
        ));
    }
    let path = hooks.join("pre-push");
    let current = held(&path)?;
    let (hooks, shown) = (quoted(&hooks), quoted(&path));
    let command =
        format!("mkdir -p {hooks} && cat > {shown} <<'{END}' && chmod 755 {shown}\n{HOOK}{END}\n");
    Ok(output(&InstallHookOutput {
        path: path.display().to_string(),
        current,
        command,
    }))
}

/// Whether `path` holds this hook already, or `github::hook::exists` when it
/// holds a hook that is not this plugin's. An older copy of this hook is
/// replaced, so it is not current.
fn held(path: &Path) -> Result<bool, PluginError> {
    if path.symlink_metadata().is_err() {
        return Ok(false);
    }
    let text = (path.is_file())
        .then(|| std::fs::read(path).ok())
        .flatten()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    if text == HOOK {
        return Ok(true);
    }
    if text.lines().any(|line| line == MARKER) {
        return Ok(false);
    }
    let shown = path.display().to_string();
    Err(error(
        "github::hook::exists",
        format!(
            "{shown} already holds a pre-push hook that is not this plugin's, and it is left as \
             it was; move it aside and run `board call github.install-hook` again"
        ),
        json!({"path": shown}),
    ))
}

/// `path` as one shell word, in single quotes.
fn quoted(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

/// What git gives its pre-push hook: the remote and the refs it pushes. The
/// URL is in `BOARD_GITHUB_PUSH_URL`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PrePushInput")]
pub(crate) struct PrePushInput {
    /// The remote's name, or the URL git was given in its place.
    remote: String,
    /// What git writes on the hook's stdin: one `<local ref> <local sha>
    /// <remote ref> <remote sha>` per line.
    refs: String,
}

/// One ref's check against one destination.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Checked")]
pub(crate) struct Checked {
    #[serde(rename = "ref")]
    reference: String,
    to: String,
    digest: String,
    note: Option<String>,
}

/// Each ref checked against each GitHub destination; empty when the push
/// goes nowhere on GitHub or sends nothing.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PrePushOutput")]
pub(crate) struct PrePushOutput {
    checked: Vec<Checked>,
}

/// Checks what git pushes, as `github.push` would check it, for each ref
/// git names. A ref deleted on the remote, whose SHA is all zeros, sends
/// nothing. The commits are those no ref at the URL git pushes to reaches,
/// so a push URL unlike the fetch URL is compared with what it holds.
pub(crate) fn pre_push(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PrePushInput { remote, refs } = PluginError::read_input("github", "pre-push", input)?;
    let top = git::top(host)?;
    let url = variable(host, PUSH_URL).unwrap_or_default();
    let destinations = destinations(host, &top, &remote, &url, true)?;
    let pushed: Vec<Vec<&str>> = (refs.lines())
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|fields| fields.len() == FIELDS && fields[1].chars().any(|digit| digit != '0'))
        .collect();
    if destinations.is_empty() || pushed.is_empty() {
        return Ok(output(&PrePushOutput { checked: vec![] }));
    }
    let personal = crossing::personal(host)?;
    let personal = &*personal;
    let from = crossing::mount(personal, &top)?;
    // One ls-remote serves every ref, so `git push --tags` makes one round
    // trip.
    let have = git::advertised(host, &top, if url.is_empty() { &remote } else { &url })?;
    let mut checked = Vec::new();
    for fields in pushed {
        let (sha, reference) = (fields[1], fields[2]);
        let parts = ref_parts(host, &top, reference, sha, &have)?;
        for to in &destinations {
            let passed = crossing::check(personal, "github.push", to, from.as_deref(), &parts)?;
            checked.push(Checked {
                reference: reference.to_owned(),
                to: to.clone(),
                digest: passed.digest,
                note: passed.note,
            });
        }
    }
    Ok(output(&PrePushOutput { checked }))
}

/// Name the boundaries that cover a write from the checkout, checking
/// nothing.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CoversInput")]
pub(crate) struct CoversInput {
    /// The github action that sends the write, such as push or pr-create.
    #[schemars(regex(pattern = "^[a-z-]+$"))]
    action: String,
    /// The remote a push names; the branch's push remote, else origin, when
    /// left out.
    #[schemars(length(min = 1))]
    remote: Option<String>,
    /// The owner/repo the write names, in place of the remote.
    #[schemars(length(min = 1))]
    repo: Option<String>,
}

/// One destination a boundary covers, and the boundaries.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Covered")]
pub(crate) struct Covered {
    to: String,
    boundaries: Vec<String>,
}

/// Each destination of the write that a boundary covers; empty when none
/// is.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CoversOutput")]
pub(crate) struct CoversOutput {
    covered: Vec<Covered>,
}

/// The destinations of a write from the checkout that a boundary covers,
/// and the boundaries, checking nothing. The destination is the repository
/// `repo` names, else the URLs of `remote` as the pre-push hook reads them.
/// With no remote, a push goes to the branch's push remote, else origin,
/// and a gh write to origin.
pub(crate) fn covers(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let CoversInput {
        action,
        remote,
        repo,
    } = PluginError::read_input("github", "covers", input)?;
    let top = git::top(host)?;
    let push = action == "push";
    let destinations = if let Some(repo) = repo {
        vec![format!("github:{}", repo.to_lowercase())]
    } else {
        let mut remote = remote.unwrap_or_default();
        if remote.is_empty() && push {
            let branch = run(host, &top, &["symbolic-ref", "-q", "HEAD"], "");
            let format = [
                "for-each-ref",
                "--format=%(push:remotename)",
                branch.stdout.trim(),
            ];
            run(host, &top, &format, "")
                .stdout
                .trim()
                .clone_into(&mut remote);
        }
        let remote = if remote.is_empty() { "origin" } else { &remote };
        destinations(host, &top, remote, "", push)?
    };
    let personal = crossing::personal(host)?;
    let personal = &*personal;
    let from = crossing::mount(personal, &top)?;
    let action = format!("github.{action}");
    let mut covered = Vec::new();
    for to in destinations {
        let boundaries = crossing::covers(personal, &action, &to, from.as_deref())?;
        if !boundaries.is_empty() {
            covered.push(Covered { to, boundaries });
        }
    }
    Ok(output(&CoversOutput { covered }))
}
