//! The record types the plugin ships. Their schemas are generated from
//! these structs; the files under `types/` are those schemas, committed.
//! They keep the bytes the Python plugin shipped, with a title on every
//! property as pydantic wrote it, so a board that enabled prompts before
//! the port needs no `plugin enable`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use board_plugin::shape::{Record, Tag};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

/// An input's name, as `{name}` names it in a prompt.
pub const INPUT: &str = "[a-z][a-z0-9_]{0,31}";

/// Work that starts in a clone of the GitHub repository whose remote is `remote`.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "InClone")]
pub struct InClone {
    /// owner/name, or an input such as {repo}.
    #[schemars(
        title = "Remote",
        regex(pattern = r"^(\{[a-z][a-z0-9_]{0,31}\}|[^/\s]+/[^/\s]+)$")
    )]
    pub remote: String,
}

/// Work that starts in a directory that is not one repository.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "InDirectory")]
pub struct InDirectory {
    /// An absolute path, a path starting ~/, or an input alone or followed by /.
    #[schemars(
        title = "Directory",
        regex(pattern = r"^((/|~/).*|\{[a-z][a-z0-9_]{0,31}\}(/.*)?)$")
    )]
    pub directory: String,
}

// Where the work starts: one of `InClone` and `InDirectory`. A comment, not
// a doc comment, which would become a description in the schema.
#[derive(Debug, JsonSchema)]
#[serde(untagged)]
#[schemars(inline)]
pub enum RunsIn {
    InClone(InClone),
    InDirectory(InDirectory),
}

/// `prompts/prompt/v1`: the frontmatter of a Markdown prompt under `prompts/`, whose body is the text to render.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "PromptRecord",
    description = "`prompts/prompt/v1`: the frontmatter of a Markdown prompt under `prompts/`, whose body is the text to render.\n\n`runs_in` says where the work starts. `inputs` maps each input's name to a\ndescription of its value; every declared input is required. Any other\nfield is refused, because a misspelt field in an instruction should fail."
)]
pub struct PromptRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    #[schemars(title = "Title", length(min = 1))]
    pub title: String,
    #[schemars(title = "Purpose", length(min = 1))]
    pub purpose: String,
    #[schemars(title = "Runs In")]
    pub runs_in: RunsIn,
    #[schemars(title = "Inputs", with = "Option<Inputs>")]
    pub inputs: Option<BTreeMap<String, String>>,
}

impl Record for PromptRecord {
    const TYPE: &'static str = "prompts/prompt/v1";
}

/// `inputs`: a description of each input by a name `INPUT` admits, and no
/// other name.
struct Inputs;

impl JsonSchema for Inputs {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Inputs")
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let pattern = format!("^{INPUT}$");
        json_schema!({
            "type": "object",
            "patternProperties": {pattern: {"type": "string"}},
            "additionalProperties": false,
        })
    }
}

/// `prompts/repos/v1`: the mounts whose prompts the board admits, at `/prompts/repos.json` in its own tree.
#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "ReposRecord",
    description = "`prompts/repos/v1`: the mounts whose prompts the board admits, at `/prompts/repos.json` in its own tree.\n\nOnly this file in the board's own working tree marks a repo, and only a mount whose pointer is in that same\ntree counts, so nothing a mounted repository commits can make its prompts admitted."
)]
pub struct ReposRecord {
    #[serde(rename = "$type")]
    #[schemars(title = "$Type")]
    pub tag: Tag<Self>,
    /// The mount of each repo whose prompts are admitted, such as /repos/notes-personal.
    #[schemars(title = "Mounts", schema_with = "mounts")]
    pub mounts: Vec<String>,
}

impl Record for ReposRecord {
    const TYPE: &'static str = "prompts/repos/v1";
}

/// `mounts`: distinct references, each a `board-ref`.
fn mounts(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "items": {"type": "string", "format": "board-ref"},
        "uniqueItems": true,
    })
}
