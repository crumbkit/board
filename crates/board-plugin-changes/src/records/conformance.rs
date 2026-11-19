//! `changes/conformance/v1`, a critic's comparison of a built page with an
//! agreed design.

use board_plugin::shape::{CommitName, Prose, Record, Ref, Tag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AnyChange;

/// `changes/conformance/v1`: what a critic with a clean context found when
/// it screenshotted the built page and compared it with one version of the
/// artifact the owner agreed.
///
/// It lives at `/changes/<name>/conformance/<n>`. The review gate passes
/// when a record at the change's head says `match` and lists no difference,
/// and otherwise shows each difference of the records at the head.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-board-folder" = "/changes/{change}/conformance/"))]
pub(crate) struct ConformanceRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    tag: Tag<Self>,
    /// The change whose build was compared.
    #[schemars(with = "Ref<AnyChange>")]
    change: String,
    /// The mockup, as the plan's Mockups list names it: a URL or a board
    /// path.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    mockup: String,
    /// The version of the mockup the build was compared with.
    #[schemars(length(min = 1))]
    version: String,
    /// The commit the screenshot was taken at.
    #[schemars(with = "CommitName")]
    head: String,
    /// What was screenshotted: the URL or file of the built page.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    page: String,
    /// Where the screenshot is saved, when it is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    screenshot: Option<String>,
    /// Whether the build matches the version: `match` only when
    /// `differences` is empty.
    verdict: Verdict,
    /// Each element of the version the build lacks or shows differently.
    differences: Vec<Difference>,
}

impl Record for ConformanceRecord {
    const TYPE: &'static str = "changes/conformance/v1";
}

/// The verdict of a comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Verdict {
    Match,
    Differs,
}

/// One element the built page lacks or shows differently from the design.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Difference")]
struct Difference {
    /// The element, such as `flow graph` or `status badge`.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    element: String,
    /// Whether the build lacks the element or shows it differently.
    kind: Gap,
    /// What the version shows.
    #[schemars(with = "Prose", extend("minLength" = 1))]
    expected: String,
    /// What the build shows, when it shows the element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Prose>")]
    built: Option<String>,
}

/// How a built element departs from the design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
enum Gap {
    Missing,
    Different,
}
