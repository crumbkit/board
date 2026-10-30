//! `plugin list` and `plugin find`.

use board_model::{Code, canonical_json};
use board_ops::{PluginEntry, PluginFindReport, PluginListReport, PluginState};
use board_plugin::{CallReport, Capability};
use board_tree::Status;
use crumbkit::help::Leaf;
use crumbkit::{Document, Listing, Prose, Row};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::Health;
use super::kit::{Printer, Refused, Said};
use crate::report::Output;
use crate::run::Page;

/// `plugin list`: as text, a line per plugin with its status, version,
/// whether the board enables it, any schema it lacks or holds differently,
/// and the names of its capabilities. Broken or missing plugins come first
/// and report a problem, so the run exits 1.
pub(crate) fn plugin_list(printer: Printer, report: &PluginListReport, page: &Page) -> Output {
    let mut plugins: Vec<&PluginEntry> = report.plugins.iter().collect();
    plugins.sort_by_key(|plugin| plugin.code().is_none());
    let rows = plugins.into_iter().map(Listed::of).collect();
    let listing = page.first_of(rows).described("plugins");
    printer.print(LIST_REPORT, Ok(listing))
}

/// A plugin as `plugin list` prints it. The fields are in name order, so
/// the JSON keeps the order it had when each row was a JSON object.
#[derive(Serialize, JsonSchema)]
struct Listed {
    /// An installed plugin's capabilities, each as an MCP Tool object.
    #[serde(skip_serializing_if = "Option::is_none")]
    capabilities: Option<Vec<Value>>,
    /// The code of the plugin's first problem, left out when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    /// What an installed plugin's manifest says the plugin does.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Whether this board's configuration enables the plugin.
    enabled: bool,
    /// Why the plugin's manifest was refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// An installed plugin's metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
    /// The plugins directory's entry, or the name the board enables.
    name: String,
    /// The names of the capabilities the plugin offers; `board plugin find
    /// --capability <name>` describes each.
    offers: Vec<String>,
    /// The program an installed plugin runs, and its arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<Vec<String>>,
    /// Each schema an enabled plugin ships that the board lacks or holds
    /// differently, as its `type`, `path` and `code`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    schemas: Vec<Value>,
    /// `ok`, or `degraded` when the plugin has a problem. Text adds the
    /// problem's code, which JSON holds in `code`.
    status: Health,
    /// An installed plugin's version.
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip)]
    line: String,
}

impl Listed {
    fn of(plugin: &PluginEntry) -> Self {
        let code = plugin.code();
        let status = Health(code.map_or(Status::Ok, Status::Degraded));
        let manifest = match &plugin.state {
            PluginState::Installed(manifest) => Some(manifest),
            PluginState::BadManifest(_) | PluginState::NotInstalled => None,
        };
        let offers: Vec<String> = (manifest.iter())
            .flat_map(|manifest| &manifest.capabilities)
            .map(|offered| offered.name.to_string())
            .collect();
        let version = manifest.map(|manifest| manifest.version.to_string());
        let line = format!(
            "{status} {}{} {}{}{}",
            plugin.name,
            version
                .as_ref()
                .map_or_else(String::new, |v| format!(" {v}")),
            enabled(plugin.enabled),
            problems(plugin),
            if offers.is_empty() {
                String::new()
            } else {
                format!(" offers {}", offers.join(", "))
            },
        );
        Self {
            capabilities: manifest.map(|manifest| {
                (manifest.capabilities.iter())
                    .map(|offered| {
                        let mut value = capability(offered);
                        value["name"] = json!(offered.name);
                        value
                    })
                    .collect()
            }),
            code: code.map(Code::as_str),
            description: manifest.map(|manifest| manifest.description.clone()),
            enabled: plugin.enabled,
            message: match &plugin.state {
                PluginState::BadManifest(error) => Some(error.to_string()),
                PluginState::Installed(_) | PluginState::NotInstalled => None,
            },
            metadata: manifest.map(|manifest| json!(manifest.metadata)),
            name: plugin.name.clone(),
            offers,
            run: manifest.map(|manifest| manifest.run.clone()),
            schemas: (plugin.schemas.iter())
                .map(|problem| {
                    json!({"type": problem.record_type.to_string(),
                        "path": problem.path, "code": problem.code.as_str()})
                })
                .collect(),
            status,
            version,
            line,
        }
    }
}

/// Each row prints whole in JSON, for the readers that parse it so, and
/// prints its own line in text unless `--long` or `--fields` asks for the
/// table.
impl Row for Listed {
    const COLUMNS: &'static [&'static str] = &["status", "name", "version", "enabled", "offers"];
    const LONG_COLUMNS: &'static [&'static str] = &["description", "message", "schemas"];
    const WHOLE: bool = true;
    const LINE: Option<&'static str> =
        Some("<STATUS> <NAME> [<VERSION>] <ENABLED>[: <PROBLEMS>][ offers <OFFERS>]");

    /// A row with a `code` reports a problem. `code` is not a column, because
    /// the text of `status` holds it, so the row says so itself (ADR-0096).
    fn is_problem(&self) -> bool {
        self.code.is_some()
    }

    fn text_line(&self) -> Option<String> {
        Some(self.line.clone())
    }
}

const LIST_REPORT: Document<Listing<Listed>> = Document::new(
    "board",
    "plugin-list-report",
    2,
    "The installed and enabled plugins.",
)
.when_empty("no plugins are installed or enabled");

pub(super) const LIST: Leaf = Leaf::new(&["plugin", "list"]).document(LIST_REPORT);

/// A refused manifest's message, or each schema problem as its code and
/// file, after `: `; empty when none.
fn problems(plugin: &PluginEntry) -> String {
    if let PluginState::BadManifest(error) = &plugin.state {
        return format!(": {error}");
    }
    let problems: Vec<String> = plugin
        .schemas
        .iter()
        .map(|problem| format!("{} {}", problem.code, problem.path))
        .collect();
    if problems.is_empty() {
        String::new()
    } else {
        format!(": {}", problems.join("; "))
    }
}

/// `plugin find`: a line or an object per matching capability.
pub(crate) fn plugin_find(printer: Printer, report: &PluginFindReport, page: &Page) -> Output {
    let rows = report.plugins.iter().flat_map(|found| {
        let manifest = &found.manifest;
        (manifest.capabilities.iter()).map(move |offered| Found {
            meta: offered.meta.clone(),
            annotations: offered.annotations.as_ref().map(|value| json!(value)),
            capability: offered.name.to_string(),
            description: offered.description.clone(),
            enabled: found.enabled,
            env: env(offered),
            input_schema: offered.input.document().clone(),
            output_schema: offered.output.document().clone(),
            plugin: manifest.name.to_string(),
            timeout: timeout(offered),
            title: offered.title.clone(),
            version: manifest.version.to_string(),
        })
    });
    let listing = page.first_of(rows.collect()).described("capabilities");
    printer.print(FIND_REPORT, Ok(listing))
}

/// A capability as `plugin find` prints it: its MCP Tool object without
/// its name, with the plugin that offers it. The fields are in name order,
/// so the JSON keeps the order it had when each row was a JSON object.
#[derive(Serialize, JsonSchema)]
struct Found {
    /// The capability's `_meta`, which `--meta` matches before the
    /// plugin's metadata.
    #[serde(rename = "_meta")]
    meta: Map<String, Value>,
    /// MCP's hints about the capability, such as `readOnlyHint`.
    #[serde(skip_serializing_if = "Option::is_none")]
    annotations: Option<Value>,
    /// The capability's name; `board call <plugin>.<capability>` runs it.
    capability: String,
    /// What the capability does.
    description: String,
    /// Whether this board's configuration enables the plugin.
    enabled: bool,
    /// The variables of Board's environment the capability also receives.
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<Vec<String>>,
    /// The JSON Schema of the input the capability takes.
    #[serde(rename = "inputSchema")]
    input_schema: Value,
    /// The JSON Schema of the output the capability returns.
    #[serde(rename = "outputSchema")]
    output_schema: Value,
    /// The plugin that offers the capability.
    plugin: String,
    /// The capability's time limit in seconds, when it sets its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<u64>,
    /// The capability's title for people.
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    /// The plugin's version.
    version: String,
}

/// Each row prints whole in JSON, for the readers that parse it so, and
/// prints its own line in text unless `--long` or `--fields` asks for the
/// table.
impl Row for Found {
    const COLUMNS: &'static [&'static str] = &["plugin", "capability", "enabled", "description"];
    const LONG_COLUMNS: &'static [&'static str] =
        &["title", "version", "annotations", "timeout", "env"];
    const WHOLE: bool = true;
    const LINE: Option<&'static str> = Some("<PLUGIN>.<CAPABILITY> <ENABLED> <DESCRIPTION>");

    fn text_line(&self) -> Option<String> {
        Some(format!(
            "{}.{} {} {}",
            self.plugin,
            self.capability,
            enabled(self.enabled),
            self.description
        ))
    }
}

const FIND_REPORT: Document<Listing<Found>> = Document::new(
    "board",
    "plugin-find-report",
    2,
    "The installed capabilities, or those that match every filter given.",
)
.when_empty("no capability is installed, or none matches every filter given");

pub(super) const FIND: Leaf = Leaf::new(&["plugin", "find"]).document(FIND_REPORT);

fn enabled(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

/// The capability as an MCP Tool object, without its name.
fn capability(capability: &Capability) -> Value {
    let mut value = json!({
        "description": capability.description,
        "inputSchema": capability.input.document(),
        "outputSchema": capability.output.document(),
        "_meta": capability.meta,
    });
    if let Some(title) = &capability.title {
        value["title"] = json!(title);
    }
    if let Some(annotations) = &capability.annotations {
        value["annotations"] = json!(annotations);
    }
    if let Some(seconds) = timeout(capability) {
        value["timeout"] = json!(seconds);
    }
    if let Some(names) = env(capability) {
        value["env"] = json!(names);
    }
    value
}

/// A capability's own time limit in seconds, when it sets one.
fn timeout(capability: &Capability) -> Option<u64> {
    capability.timeout.map(|limit| limit.as_secs())
}

/// The variables a capability's `env` names, when it names any.
fn env(capability: &Capability) -> Option<Vec<String>> {
    (!capability.env.is_empty()).then(|| {
        (capability.env.iter())
            .map(|name| name.as_str().to_owned())
            .collect()
    })
}

/// `call`: the output as canonical JSON, and as `data` with `--json`.
pub(crate) fn call(printer: Printer, report: &CallReport) -> Output {
    let value = json!({
        "plugin": report.plugin,
        "capability": report.capability,
        "output": report.output,
    });
    let answer = Prose::new(Said::new(
        value,
        canonical_json(&report.output).as_str().to_owned(),
    ));
    printer.noted(CALL_REPORT, answer, Vec::new())
}

pub(crate) const CALL_REPORT: Document<Prose<Said, Refused>> = Document::new(
    "board",
    "call-report",
    2,
    "What a plugin's capability returned.",
);

pub(super) const CALL: Leaf = Leaf::new(&["call"]).document(CALL_REPORT);
