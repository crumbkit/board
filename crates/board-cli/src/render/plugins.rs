//! `plugin list` and `plugin find`.

use std::fmt::Write as _;

use board_model::{Fields, Refusal, canonical_json};
use board_ops::{CallFailed, Called, Plugin, PluginFindReport, PluginListReport, PluginState};
use board_plugin::{Capability, Manifest, PluginName};
use serde_json::{Value, json};

use super::{Format, Shown, document, refused};
use crate::report::Output;

/// `plugin list`: as text, a line per plugin with its status, version,
/// whether the board enables it and any schema it lacks or holds differently,
/// then a line per capability. Exit 1 when a
/// plugin is broken or missing.
pub(crate) fn plugin_list(
    format: Format,
    report: &PluginListReport,
    limit: Option<usize>,
) -> Output {
    let code = u8::from(report.degraded());
    // A line per plugin and per capability, so a cut can fall within one
    // plugin's capabilities.
    let lines = report
        .plugins
        .iter()
        .map(|plugin| 1 + offered(plugin).len());
    let shown = Shown::of(lines.sum(), limit);
    let mut budget = shown.shown;
    let plugins: Vec<(&Plugin, &[Capability])> = report
        .plugins
        .iter()
        .map_while(|plugin| {
            let capabilities = offered(plugin);
            let take = capabilities.len().min(budget.checked_sub(1)?);
            budget -= 1 + take;
            Some((plugin, &capabilities[..take]))
        })
        .collect();
    match format {
        Format::Text => {
            let mut text = String::new();
            for (plugin, capabilities) in &plugins {
                let status = plugin
                    .code()
                    .map_or_else(|| "ok".to_owned(), |code| format!("degraded {code}"));
                let version = match &plugin.state {
                    PluginState::Installed(manifest) => format!(" {}", manifest.version),
                    PluginState::BadManifest(_) | PluginState::NotInstalled => String::new(),
                };
                let _ = writeln!(
                    text,
                    "{status} {}{version} {}{}",
                    plugin.name,
                    enabled(plugin.enabled),
                    problems(plugin)
                );
                for capability in *capabilities {
                    let _ = writeln!(text, "  {} {}", capability.name, capability.description);
                }
            }
            Output {
                stdout: text.into_bytes(),
                stderr: shown
                    .note(
                        "plugins and capabilities",
                        "no plugins are installed or enabled",
                    )
                    .into_bytes(),
                code,
            }
        }
        Format::Json => {
            let plugins: Vec<Value> = plugins
                .iter()
                .map(|(plugin, capabilities)| {
                    let mut value = match &plugin.state {
                        PluginState::Installed(manifest) => installed(manifest, capabilities),
                        PluginState::BadManifest(error) => json!({"message": error.to_string()}),
                        PluginState::NotInstalled => json!({}),
                    };
                    value["name"] = json!(plugin.name);
                    value["enabled"] = json!(plugin.enabled);
                    if !plugin.schemas.is_empty() {
                        value["schemas"] = plugin
                            .schemas
                            .iter()
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
                    value
                })
                .collect();
            document(
                &shown.mark(json!({
                    "$type": "board/plugin-list-report/v1",
                    "degraded": report.degraded(),
                    "plugins": plugins,
                })),
                code,
            )
        }
    }
}

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
pub(crate) fn plugin_find(
    format: Format,
    report: &PluginFindReport,
    limit: Option<usize>,
) -> Output {
    let mut rows: Vec<_> = report
        .plugins
        .iter()
        .flat_map(|found| {
            found
                .manifest
                .capabilities
                .iter()
                .map(move |capability| (&found.manifest, found.enabled, capability))
        })
        .collect();
    let shown = Shown::of(rows.len(), limit);
    rows.truncate(shown.shown);
    match format {
        Format::Text => {
            let mut text = String::new();
            for (manifest, on, capability) in rows {
                let _ = writeln!(
                    text,
                    "{}.{} {} {}",
                    manifest.name,
                    capability.name,
                    enabled(on),
                    capability.description
                );
            }
            Output {
                stdout: text.into_bytes(),
                stderr: shown
                    .note(
                        "capabilities",
                        "no installed capability matches every filter",
                    )
                    .into_bytes(),
                ..Output::default()
            }
        }
        Format::Json => {
            let capabilities: Vec<Value> = rows
                .into_iter()
                .map(|(manifest, on, capability)| {
                    let mut value = self::capability(capability);
                    value["capability"] = json!(capability.name);
                    value["plugin"] = json!(manifest.name);
                    value["version"] = json!(manifest.version);
                    value["enabled"] = json!(on);
                    value
                })
                .collect();
            document(
                &shown.mark(json!({
                    "$type": "board/plugin-find-report/v1",
                    "capabilities": capabilities,
                })),
                0,
            )
        }
    }
}

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

/// `call`: the output as JSON, or wrapped in `board/call-report/v1`.
pub(crate) fn call(format: Format, called: &Called) -> Output {
    let report = &called.report;
    let fallbacks: &[PluginName] = if called.fallback {
        std::slice::from_ref(&report.plugin)
    } else {
        &[]
    };
    let output = match format {
        Format::Text => Output {
            stdout: canonical_json(&report.output),
            ..Output::default()
        },
        Format::Json => document(
            &json!({
                "$type": "board/call-report/v1",
                "plugin": report.plugin,
                "capability": report.capability,
                "output": report.output,
            }),
            0,
        ),
    };
    temporary(output, fallbacks)
}

/// `output` with a line on stderr for each plugin whose database is a
/// temporary file though the index is kept.
pub(crate) fn temporary(mut output: Output, fallbacks: &[PluginName]) -> Output {
    for plugin in fallbacks {
        output.stderr.extend_from_slice(
            format!(
                "note: plugin {plugin}'s database is a temporary file for this run: \
                 no directory for it can be made beside the index\n"
            )
            .as_bytes(),
        );
    }
    output
}

/// A refused call, with its typed fields; as text, the end of the plugin's
/// stderr follows the error. A plugin's own code has no hint from Board.
pub(crate) fn call_error(format: Format, error: &CallFailed) -> Output {
    let fields = error.fields();
    let named = |name: &str| fields.get(name).and_then(Value::as_str).map(str::to_owned);
    // An ambiguous capability's hint shows a call naming the first plugin.
    let first = fields
        .get("plugins")
        .and_then(|plugins| plugins.get(0))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let known = Fields {
        plugin: named("plugin").or(first),
        capability: named("capability"),
        ..Fields::default()
    };
    let refusal = match error.code() {
        Some(code) => Refusal {
            fields,
            ..Refusal::new(code, error, &known)
        },
        None => Refusal::plugin(error.code_text().to_owned(), error.to_string(), fields),
    };
    let stderr = error.stderr().unwrap_or_default().to_vec();
    refused(format, &Refusal { stderr, ..refusal })
}
