//! The checks of the `mockups` kind: the UX section lists each mockup, an
//! owner decision agrees it, the plan names its version, and the review
//! holds a record that the built page matches that version.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::{
    NO_HEAD, Observed, objects, owner_signed, pattern, same_commit, signed_decisions, strings,
};
use crate::plan::answers::Ux;

/// The record type of a critic's comparison of a build with a design.
pub(crate) const CONFORMANCE_TYPE: &str = "changes/conformance/v1";
/// The record type of the owner's answer to a question.
const ANSWER_TYPE: &str = "plans/answer/v1";

/// How a UX section says it lists no mockups, at its start, without case.
static NO_UI: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^no ui change\b[\s.:,;-]*\w"));
/// A mockup an item of the UX section's Mockups list names: its first URL,
/// or else its first board path in inline code.
static MOCKUP_URL: LazyLock<Regex> = LazyLock::new(|| pattern(r"https?://[^\s)>\]]+"));
static MOCKUP_PATH: LazyLock<Regex> = LazyLock::new(|| pattern(r"`(/[^`\s]+)`"));
static LIST_ITEM: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?m)^\s*(?:[-*+]|\d+[.)])\s+(\S.*)$"));

/// The list items under the `heading` subsection of the UX section, or
/// none when UX holds no such heading.
fn under_ux(found: &BTreeMap<String, String>, heading: &str) -> Vec<String> {
    let named = pattern(&format!(r"(?im)^#{{1,6}}\s+{heading}\s*$"));
    if !named.is_match(found.get("ux").map_or("", String::as_str)) {
        return Vec::new();
    }
    let text = found.get(heading).map_or("", String::as_str);
    (LIST_ITEM.captures_iter(text))
        .filter_map(|captures| Some(captures.get(1)?.as_str().trim().to_owned()))
        .collect()
}

/// The artifact a Mockups item names: its first URL, or else its first
/// board path in inline code.
fn named_in(item: &str) -> Option<String> {
    if let Some(url) = MOCKUP_URL.find(item) {
        return Some(
            url.as_str()
                .trim_end_matches(['.', ',', ';', ':'])
                .to_owned(),
        );
    }
    let path = MOCKUP_PATH.captures(item)?.get(1)?;
    Some(path.as_str().to_owned())
}

pub(crate) fn mockups(found: &BTreeMap<String, String>) -> Vec<String> {
    let mut named = Vec::new();
    for item in under_ux(found, "mockups") {
        if let Some(name) = named_in(&item)
            && !named.contains(&name)
        {
            named.push(name);
        }
    }
    named
}

fn named_by_decisions(observed: &Observed) -> BTreeSet<&str> {
    (signed_decisions(observed))
        .flat_map(|(_, body)| strings(body, "urls").chain(strings(body, "refs")))
        .collect()
}

pub(crate) fn ux_missing(observed: &Observed, found: &BTreeMap<String, String>) -> Option<String> {
    // A plan whose typed answer is mockups lists them, whatever its text says.
    if observed.answers.ux != Some(Ux::Mockups)
        && NO_UI.is_match(found.get("ux").map_or("", String::as_str))
    {
        return None;
    }
    let items = under_ux(found, "mockups");
    if items.is_empty() {
        return Some(if observed.answers.ux == Some(Ux::Mockups) {
            "the plan answers `ux` mockups and its UX section lists none under a ### Mockups \
             heading"
        } else {
            "the UX section neither starts with No UI change and a reason nor lists mockups under \
             a ### Mockups heading"
        }
        .to_owned());
    }
    if let Some(unnamed) = items.iter().find(|item| named_in(item).is_none()) {
        return Some(format!(
            "this item of the Mockups list names no artifact by URL or board path: {unnamed}"
        ));
    }
    let agreed = named_by_decisions(observed);
    let unagreed: Vec<String> = (mockups(found).into_iter())
        .filter(|mockup| {
            !agreed.contains(mockup.as_str())
                && !artifact_of(mockup).is_some_and(|artifact| agreed.contains(artifact))
        })
        .collect();
    (!unagreed.is_empty()).then(|| {
        format!(
            "these mockups have no attached plans/decision/v1 that names them in its urls or \
             refs: {}",
            unagreed.join(", ")
        )
    })
}

/// The artifact a `/artifacts/<id>/versions/<id>` path names a version of,
/// which a decision may name in place of the version.
fn artifact_of(path: &str) -> Option<&str> {
    let (artifact, _) = path.split_once("/versions/")?;
    artifact.starts_with("/artifacts/").then_some(artifact)
}

pub(super) fn neighbours_missing(found: &BTreeMap<String, String>) -> Option<String> {
    if under_ux(found, "mockups").is_empty() || !under_ux(found, "neighbours").is_empty() {
        return None;
    }
    Some(
        "the UX section lists mockups but has no ### Neighbours heading under it with a list: \
         name the open changes and artifacts in the same area, and the components and flows the \
         design reuses"
            .to_owned(),
    )
}

/// The version an item of the Mockups list names: the last part of a
/// `/versions/<id>` path, or the id after the word `version`, which holds a
/// digit so that `version picker` or `version: https://` names none.
static VERSION_PATH: LazyLock<Regex> = LazyLock::new(|| pattern(r"/versions/([A-Za-z0-9_-]+)"));
static VERSION_WORD: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)\bversion[ :]+`?([A-Za-z0-9_-]*[0-9][A-Za-z0-9_-]*)`?"));

fn version_in(item: &str) -> Option<String> {
    let found = VERSION_PATH
        .captures(item)
        .or_else(|| VERSION_WORD.captures(item))?;
    Some(found.get(1)?.as_str().to_owned())
}

/// Each mockup the plan lists with the version it names.
fn versioned(found: &BTreeMap<String, String>) -> Vec<(String, String)> {
    (under_ux(found, "mockups").iter())
        .filter_map(|item| Some((named_in(item)?, version_in(item)?)))
        .collect()
}

/// An item of the Mockups list that names an artifact and no version of
/// it.
pub(super) fn versions_missing(found: &BTreeMap<String, String>) -> Option<String> {
    let items = under_ux(found, "mockups");
    let bare = items
        .iter()
        .find(|item| named_in(item).is_some() && version_in(item).is_none())?;
    Some(format!(
        "this item of the Mockups list names no version of its artifact: {bare}; add `version \
         <id>` to it, or name a /artifacts/<id>/versions/<id> path"
    ))
}

/// How one difference reads: the element, how it departs and what each
/// side shows.
fn difference(each: &Map<String, Value>) -> String {
    let text = |key: &str| each.get(key).and_then(Value::as_str).unwrap_or("?");
    let built = (each.get("built").and_then(Value::as_str))
        .map_or_else(String::new, |built| format!(", built {built}"));
    format!(
        "{} is {}, expected {}{built}",
        text("element"),
        text("kind"),
        text("expected")
    )
}

/// Whether the owner has answered the question a difference names, so the
/// difference no longer blocks: an attached plans/answer/v1 for it, written
/// by a person and no hook.
fn cleared(observed: &Observed, each: &Map<String, Value>) -> bool {
    let Some(question) = each.get("question").and_then(Value::as_str) else {
        return false;
    };
    (observed.records.of_type(ANSWER_TYPE)).any(|(_, answer)| {
        answer.get("question").and_then(Value::as_str) == Some(question) && owner_signed(answer)
    })
}

/// A sortable form of a UTC timestamp ending in `Z`: the whole seconds, then
/// the fraction with its trailing zeros dropped, so `10:00:00.5Z` sorts after
/// `10:00:00Z`. A record with no timestamp sorts first.
fn instant(at: Option<&str>) -> (String, String) {
    let at = at.unwrap_or_default().trim_end_matches('Z');
    let (whole, fraction) = at.split_once('.').unwrap_or((at, ""));
    (whole.to_owned(), fraction.trim_end_matches('0').to_owned())
}

/// The record that decides a mockup version: the newest at the head, by
/// its `at`, and by its path when two share one.
fn latest<'a>(
    observed: &'a Observed,
    head: &str,
    mockup: &str,
    version: &str,
) -> Option<&'a Map<String, Value>> {
    let text = |body: &'a Map<String, Value>, key: &str| {
        body.get(key).and_then(Value::as_str).map(str::to_owned)
    };
    (observed.records.of_type(CONFORMANCE_TYPE))
        .filter(|(_, body)| {
            same_commit(body.get("head"), head)
                && text(body, "mockup").as_deref() == Some(mockup)
                && text(body, "version").as_deref() == Some(version)
        })
        .max_by_key(|(path, body)| (instant(text(body, "at").as_deref()), *path))
        .map(|(_, body)| body)
}

/// Each mockup the plan lists lacks a version, or lacks a newest record at
/// the head that says the build matches it. A difference the owner answered
/// in a plans/answer/v1 does not count. The detail lists every difference
/// left.
pub(super) fn built_missing(
    observed: &Observed,
    found: &BTreeMap<String, String>,
) -> Option<String> {
    if let Some(bare) = versions_missing(found) {
        return Some(bare);
    }
    let wanted = versioned(found);
    if wanted.is_empty() {
        return Some(
            "the plan lists no mockup that names a version, so there is nothing to compare the \
             build with"
                .to_owned(),
        );
    }
    let Some(head) = observed.change.head.as_deref() else {
        return Some(NO_HEAD.to_owned());
    };
    let mut unmet = Vec::new();
    for (mockup, version) in &wanted {
        let Some(record) = latest(observed, head, mockup, version) else {
            unmet.push(format!(
                "no {CONFORMANCE_TYPE} record says the built page matches {mockup} version \
                 {version} at {head}"
            ));
            continue;
        };
        let listed: Vec<String> = (objects(record, "differences"))
            .filter(|each| !cleared(observed, each))
            .map(difference)
            .collect();
        let says_match = record.get("verdict").and_then(Value::as_str) == Some("match");
        if !listed.is_empty() {
            unmet.push(format!(
                "the built page differs from {mockup} version {version}: {}",
                listed.join("; ")
            ));
        } else if !says_match && objects(record, "differences").next().is_none() {
            unmet.push(format!(
                "the newest record says the built page differs from {mockup} version {version} \
                 and lists no element; list each one"
            ));
        }
    }
    (!unmet.is_empty()).then(|| unmet.join("; "))
}
