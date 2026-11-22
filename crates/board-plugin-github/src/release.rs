//! `release-create`: a GitHub release of a tag GitHub has, published only
//! once the boundary passes its tag, title, notes and asset names and every
//! file is uploaded to its draft. A rerun resumes the draft it finds.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use board_ops::{Host, PluginError};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::gh;
use crate::git::error;
use crate::parts::Parts;
use crate::send::{Delivered, Origin, origin, output};

/// How long one `gh release upload` may run.
const UPLOAD: Duration = Duration::from_mins(10);
/// The most files a release takes.
const MOST: usize = 100;
/// GitHub's limit on one asset, in bytes: 2 GiB.
const LARGEST: u64 = 2 << 30;
/// What gh prints when a tag has no release.
const NO_RELEASE: &str = "release not found";
/// An asset name GitHub publishes unchanged.
static NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9A-Za-z][0-9A-Za-z._+-]*$").expect("the pattern is a constant")
});

/// Publish a GitHub release of a tag GitHub has, once the boundary passes
/// it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ReleaseCreateInput")]
pub(crate) struct ReleaseCreateInput {
    /// The tag to release, which must already be on GitHub, such as v0.1.0.
    #[schemars(regex(pattern = r"^[0-9A-Za-z][0-9A-Za-z._+-]{0,99}$"))]
    tag: String,
    /// The release's title; the tag's name when left out.
    #[schemars(length(min = 1))]
    title: Option<String>,
    /// The release notes, in Markdown.
    notes: String,
    /// A directory whose regular files, directly inside it, are uploaded as
    /// the release's assets; relative to the directory Board was started
    /// in. Left out, the release has no files.
    #[schemars(length(min = 1))]
    assets: Option<String>,
    /// Ask the owner to let this exact content through when a check fails,
    /// which files one request the owner answers; left out, an earlier answer
    /// still applies but no request is filed.
    ask_owner: Option<bool>,
}

/// The release published: `sent` is its page, `assets` the file names and
/// `resumed` whether a draft from an earlier run was found.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ReleaseCreateOutput")]
pub(crate) struct Released {
    digest: String,
    crossing: String,
    note: Option<String>,
    sent: String,
    tag: String,
    assets: Vec<String>,
    resumed: bool,
}

/// One file to upload.
struct Asset {
    name: String,
    path: PathBuf,
    size: u64,
}

/// What `gh release view --json` prints of a draft's assets.
#[derive(Deserialize)]
struct Viewed {
    #[serde(rename = "isDraft")]
    draft: bool,
    #[serde(default)]
    assets: Vec<Uploaded>,
}

#[derive(Deserialize)]
struct Uploaded {
    name: String,
    size: u64,
    state: String,
}

/// Drafts the release, uploads the files the draft lacks, then publishes
/// it, once the tag is on GitHub, no published release has it and the
/// boundary passes the tag, title, notes and asset names.
pub(crate) fn release_create(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let ReleaseCreateInput {
        tag,
        title,
        notes,
        assets,
        ask_owner,
    } = PluginError::read_input("github", "release-create", input)?;
    let files = match assets {
        Some(directory) => listed(host, &directory)?,
        None => Vec::new(),
    };
    let place = origin(host)?;
    tagged(host, &place, &tag)?;
    let draft = viewed(host, &place, &tag)?;
    let title = title.unwrap_or_else(|| tag.clone());
    let names: Vec<String> = files.iter().map(|file| file.name.clone()).collect();
    let parts = Parts::from([
        ("release.tag".to_owned(), tag.clone()),
        ("release.title".to_owned(), title.clone()),
        ("release.notes".to_owned(), notes.clone()),
        ("release.assets".to_owned(), names.join("\n")),
    ]);
    let passed = place.check_asking("release-create", &parts, ask_owner == Some(true))?;
    let repository = format!("{}/{}", place.owner, place.repo);
    let release = |verb: &str, rest: &[&str]| -> Vec<String> {
        ["release", verb, &tag, "--repo", &repository]
            .into_iter()
            .chain(rest.iter().copied())
            .map(str::to_owned)
            .collect()
    };
    let at = Some(place.top.as_path());
    let resumed = draft.is_some();
    let uploaded = draft.map(|draft| draft.assets).unwrap_or_default();
    if !resumed {
        // `--verify-tag` makes gh refuse to create the tag.
        let rest = [
            "--verify-tag",
            "--draft",
            "--title",
            &title,
            "--notes-file",
            "-",
        ];
        gh::send(
            host,
            &release("create", &rest),
            &notes,
            "gh release create",
            at,
        )?;
    }
    upload(host, &place, &tag, &files, &uploaded)?;
    let printed = gh::send(
        host,
        &release("edit", &["--draft=false"]),
        "",
        "gh release edit",
        at,
    )?;
    let url = (printed.split_whitespace())
        .rfind(|word| word.starts_with("https://"))
        .map_or_else(
            || format!("https://github.com/{repository}/releases/tag/{tag}"),
            str::to_owned,
        );
    let Delivered {
        digest,
        crossing,
        note,
        sent,
    } = place.sent(passed, url)?;
    Ok(output(&Released {
        digest,
        crossing,
        note,
        sent,
        tag,
        assets: names,
        resumed,
    }))
}

/// `github::release::no_tag` unless GitHub has the tag `tag`.
fn tagged(host: &dyn Host, place: &Origin<'_>, tag: &str) -> Result<(), PluginError> {
    let endpoint = format!("repos/{}/{}/git/ref/tags/{tag}", place.owner, place.repo);
    if gh::api(host, &endpoint)?.is_some() {
        return Ok(());
    }
    Err(error(
        "github::release::no_tag",
        format!(
            "GitHub has no tag {tag} in {}/{}, and release-create never makes one, so nothing \
             was released; run `board call github.tag --name {tag} --target <sha> --message \
             <message>` first",
            place.owner, place.repo
        ),
        json!({"tag": tag}),
    ))
}

/// Uploads each of `files` to the draft of `tag` but those `uploaded`
/// already holds with the same name and size, each within 600 seconds.
fn upload(
    host: &dyn Host,
    place: &Origin<'_>,
    tag: &str,
    files: &[Asset],
    uploaded: &[Uploaded],
) -> Result<(), PluginError> {
    let repository = format!("{}/{}", place.owner, place.repo);
    for file in files {
        let done = (uploaded.iter()).any(|there| {
            there.name == file.name && there.size == file.size && there.state == "uploaded"
        });
        if done {
            continue;
        }
        // The release is still a draft, so replacing a file of the same name
        // changes nothing anyone sees.
        let path = file.path.to_string_lossy();
        let arguments: Vec<String> = ["release", "upload", tag, &path, "--repo", &repository]
            .into_iter()
            .chain(["--clobber"])
            .map(str::to_owned)
            .collect();
        let at = Some(place.top.as_path());
        gh::send_within(host, &arguments, "", "gh release upload", at, UPLOAD)?;
    }
    Ok(())
}

/// The draft release of `tag`, none when the tag has no release, or
/// `github::release::exists` when its release is published.
fn viewed(host: &dyn Host, place: &Origin<'_>, tag: &str) -> Result<Option<Viewed>, PluginError> {
    let repository = format!("{}/{}", place.owner, place.repo);
    let arguments = ["release", "view", tag, "--repo", &repository]
        .into_iter()
        .chain(["--json", "isDraft,assets,url"])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let named = "gh release view";
    let at = Some(place.top.as_path());
    let exited = gh::exit(host, &arguments, "", named, at, gh::TIMEOUT)?;
    let stderr = String::from_utf8_lossy(&exited.stderr);
    if exited.code != Some(0) {
        if stderr.contains(NO_RELEASE) {
            return Ok(None);
        }
        return Err(gh::failed(
            &format!("release view {tag}"),
            &format!("failed: {}", stderr.trim()),
            "check gh on PATH and `gh auth status`",
        ));
    }
    let Ok(found) = serde_json::from_slice::<Viewed>(&exited.stdout) else {
        return Err(gh::failed(
            &format!("release view {tag}"),
            "printed no release gh describes",
            "check gh's version",
        ));
    };
    if !found.draft {
        return Err(error(
            "github::release::exists",
            format!(
                "{tag} already has a published release, which release-create never changes, so \
                 nothing was sent; release the next version under a new tag"
            ),
            json!({"tag": tag}),
        ));
    }
    Ok(Some(found))
}

/// The files directly inside `directory`, sorted by name, or
/// `github::release::asset_refused` naming what cannot be published as it
/// is: an entry that is not a regular file, a name GitHub would change, a
/// file over 2 GiB, or more than 100 files.
fn listed(host: &dyn Host, directory: &str) -> Result<Vec<Asset>, PluginError> {
    let path = match &host.settings().caller {
        Some(caller) => caller.join(directory),
        None => PathBuf::from(directory),
    };
    let refused = |why: String| {
        error(
            "github::release::asset_refused",
            format!(
                "{why}, so nothing was released; fix the directory --assets names and run the \
                 command again"
            ),
            json!({"assets": path.display().to_string()}),
        )
    };
    let entries = std::fs::read_dir(&path).map_err(|cause| {
        refused(format!(
            "{} cannot be read as a directory: {cause}",
            path.display()
        ))
    })?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|cause| refused(format!("{} cannot be read: {cause}", path.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let at = entry.path();
        let kind = at
            .symlink_metadata()
            .map_err(|cause| refused(format!("{name} cannot be read: {cause}")))?;
        if !kind.file_type().is_file() {
            return Err(refused(format!(
                "{name} in {} is not a regular file; only regular files are uploaded, so move it \
                 out",
                path.display()
            )));
        }
        if !NAME.is_match(&name) {
            return Err(refused(format!(
                "{name} is not a name GitHub keeps as it is; name each file with letters, digits, \
                 `.`, `_`, `+` and `-`, starting with a letter or digit"
            )));
        }
        if kind.len() > LARGEST {
            return Err(refused(format!(
                "{name} holds {} bytes, more than GitHub's 2 GiB for one file",
                kind.len()
            )));
        }
        files.push(Asset {
            name,
            path: at,
            size: kind.len(),
        });
    }
    if files.len() > MOST {
        return Err(refused(format!(
            "{} holds {} files, more than the {MOST} a release takes",
            path.display(),
            files.len()
        )));
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}
