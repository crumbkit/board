//! `plugin list` and `plugin find`.

use board_model::canonical_json;
use board_ops::{Called, Plugin, PluginFindReport, PluginListReport, PluginState};
use board_plugin::{Capability, Manifest, PluginName};
use crumbkit::help::Leaf;
use crumbkit::{Document, Listing, Prose};
use serde_json::{Value, json};

use super::kit::{Flagged, Note, Printer, Refused, Said, Whole};
use crate::report::Output;
use crate::run::Page;

/// `plugin list`: as text, a line per plugin with its status, version,
/// whether the board enables it, any schema it lacks or holds differently,
/// and the names of its capabilities. Broken or missing plugins come first
/// and report a problem, so the run exits 1.
pub(crate) fn plugin_list(printer: &mut Printer, report: &PluginListReport, page: &Page) -> Output {
    let mut plugins: Vec<&Plugin> = report.plugins.iter().collect();
    plugins.sort_by_key(|plugin| plugin.code().is_none());
    let rows = plugins.into_iter().map(|plugin| {
        let status = plugin
            .code()
            .map_or_else(|| "ok".to_owned(), |code| format!("degraded {code}"));
        let capabilities = offered(plugin);
        let version = match &plugin.state {
            PluginState::Installed(manifest) => format!(" {}", manifest.version),
            PluginState::BadManifest(_) | PluginState::NotInstalled => String::new(),
        };
        let names: Vec<&str> = capabilities
            .iter()
            .map(|offered| offered.name.as_str())
            .collect();
        let offers = if names.is_empty() {
            String::new()
        } else {
            format!(" offers {}", names.join(", "))
        };
        let line = format!(
            "{status} {}{version} {}{}{offers}",
            plugin.name,
            enabled(plugin.enabled),
            problems(plugin)
        );
        let mut value = match &plugin.state {
            PluginState::Installed(manifest) => installed(manifest, capabilities),
            PluginState::BadManifest(error) => json!({"message": error.to_string()}),
            PluginState::NotInstalled => json!({}),
        };
        value["name"] = json!(plugin.name);
        value["enabled"] = json!(plugin.enabled);
        if !plugin.schemas.is_empty() {
            value["schemas"] = (plugin.schemas.iter())
                .map(|problem| {
                    json!({"type": problem.record_type.to_string(),
                        "path": problem.path, "code": problem.code.as_str()})
                })
                .collect();
        }
        match plugin.code() {
            None => value["status"] = json!("ok"),
            Some(code) => {
                value["status"] = json!("degraded");
                value["code"] = json!(code.as_str());
            }
        }
        Flagged(Whole::lined(value, line))
    });
    let listing = page.first_of(rows.collect()).described("plugins");
    printer.print(LIST_REPORT, Ok(listing))
}

const LIST_REPORT: Document<Listing<Flagged>> = Document::new(
    "board",
    "plugin-list-report",
    2,
    "The installed and enabled plugins.",
)
.when_empty("no plugins are installed or enabled");

pub(super) const LIST: Leaf = Leaf::new(&["plugin", "list"]).document(LIST_REPORT);

/// A refused manifest's message, or each schema problem as its code and
/// file, after `: `; empty when none.
fn problems(plugin: &Plugin) -> String {
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

/// The capabilities a plugin offers: none unless it is installed.
fn offered(plugin: &Plugin) -> &[Capability] {
    match &plugin.state {
        PluginState::Installed(manifest) => &manifest.capabilities,
        PluginState::BadManifest(_) | PluginState::NotInstalled => &[],
    }
}

/// `plugin find`: a line or an object per matching capability.
pub(crate) fn plugin_find(printer: &mut Printer, report: &PluginFindReport, page: &Page) -> Output {
    let rows = report.plugins.iter().flat_map(|found| {
        let manifest = &found.manifest;
        manifest.capabilities.iter().map(move |offered| {
            let line = format!(
                "{}.{} {} {}",
                manifest.name,
                offered.name,
                enabled(found.enabled),
                offered.description
            );
            let mut value = capability(offered);
            value["capability"] = json!(offered.name);
            value["plugin"] = json!(manifest.name);
            value["version"] = json!(manifest.version);
            value["enabled"] = json!(found.enabled);
            Whole::lined(value, line)
        })
    });
    let listing = page.first_of(rows.collect()).described("capabilities");
    printer.print(FIND_REPORT, Ok(listing))
}

const FIND_REPORT: Document<Listing<Whole>> = Document::new(
    "board",
    "plugin-find-report",
    2,
    "The capabilities that match every filter.",
)
.when_empty("no installed capability matches every filter");

pub(super) const FIND: Leaf = Leaf::new(&["plugin", "find"]).document(FIND_REPORT);

fn enabled(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

/// The manifest, with only `capabilities` of its own.
fn installed(manifest: &Manifest, capabilities: &[Capability]) -> Value {
    json!({
        "version": manifest.version,
        "description": manifest.description,
        "run": manifest.run,
        "metadata": manifest.metadata,
        "extract": manifest.extract,
        "capabilities": capabilities
            .iter()
            .map(|offered| {
                let mut value = capability(offered);
                value["name"] = json!(offered.name);
                value
            })
            .collect::<Vec<_>>(),
    })
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
    value
}

/// `call`: the output as canonical JSON, and as `data` with `--json`.
pub(crate) fn call(printer: &mut Printer, called: &Called) -> Output {
    let report = &called.report;
    let value = json!({
        "plugin": report.plugin,
        "capability": report.capability,
        "output": report.output,
    });
    let answer = Prose::new(Said::new(
        value,
        canonical_json(&report.output).as_str().to_owned(),
    ));
    let fallbacks = if called.fallback {
        std::slice::from_ref(&report.plugin)
    } else {
        &[]
    };
    let notes = temporary(fallbacks).into_iter().map(Note::Always);
    printer.noted(CALL_REPORT, answer, notes.collect())
}

const CALL_REPORT: Document<Prose<Said, Refused>> = Document::new(
    "board",
    "call-report",
    2,
    "What a plugin's capability returned.",
);

pub(super) const CALL: Leaf = Leaf::new(&["call"]).document(CALL_REPORT);

/// A note for each plugin whose database is a temporary file though the
/// index is kept.
pub(crate) fn temporary(fallbacks: &[PluginName]) -> Vec<String> {
    (fallbacks.iter())
        .map(|plugin| {
            format!(
                "note: plugin {plugin}'s database is a temporary file for this run: \
                 no directory for it can be made beside the index"
            )
        })
        .collect()
}
