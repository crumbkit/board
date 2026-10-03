//! `plugin list` and `plugin find`.

use std::fmt::Write as _;

use board_model::canonical_json;
use board_ops::{CallFailed, Called, PluginFindReport, PluginListReport, PluginState};
use board_plugin::{Capability, Manifest, PluginName};
use serde_json::{Value, json};

use super::{Format, document};
use crate::report::Output;

/// `plugin list`: as text, a line per plugin with its status, version and
/// whether the board enables it, then a line per capability. Exit 1 when a
/// plugin is broken or missing.
pub(crate) fn plugin_list(format: Format, report: &PluginListReport) -> Output {
    let code = u8::from(report.degraded());
    match format {
        Format::Text => {
            let mut text = String::new();
            for plugin in &report.plugins {
                let status = plugin
                    .code()
                    .map_or_else(|| "ok".to_owned(), |code| format!("degraded {code}"));
                let version = match &plugin.state {
                    PluginState::Installed(manifest) => format!(" {}", manifest.version),
                    PluginState::BadManifest(_) | PluginState::NotInstalled => String::new(),
                };
                let _ = writeln!(
                    text,
                    "{status} {}{version} {}",
                    plugin.name,
                    enabled(plugin.enabled)
                );
                if let PluginState::Installed(manifest) = &plugin.state {
                    for capability in &manifest.capabilities {
                        let _ = writeln!(text, "  {} {}", capability.name, capability.description);
                    }
                }
            }
            Output {
                stdout: text.into_bytes(),
                code,
                ..Output::default()
            }
        }
        Format::Json => {
            let plugins: Vec<Value> = report
                .plugins
                .iter()
                .map(|plugin| {
                    let mut value = match &plugin.state {
                        PluginState::Installed(manifest) => installed(manifest),
                        PluginState::BadManifest(error) => json!({"message": error.to_string()}),
                        PluginState::NotInstalled => json!({}),
                    };
                    value["name"] = json!(plugin.name);
                    value["enabled"] = json!(plugin.enabled);
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
                &json!({
                    "$type": "board/plugin-list-report/v1",
                    "degraded": report.degraded(),
                    "plugins": plugins,
                }),
                code,
            )
        }
    }
}

/// `plugin find`: a line or an object per matching capability.
pub(crate) fn plugin_find(format: Format, report: &PluginFindReport) -> Output {
    let rows = report.plugins.iter().flat_map(|found| {
        found
            .manifest
            .capabilities
            .iter()
            .map(move |capability| (&found.manifest, found.enabled, capability))
    });
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
                ..Output::default()
            }
        }
        Format::Json => {
            let capabilities: Vec<Value> = rows
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
                &json!({
                    "$type": "board/plugin-find-report/v1",
                    "capabilities": capabilities,
                }),
                0,
            )
        }
    }
}

fn enabled(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

fn installed(manifest: &Manifest) -> Value {
    json!({
        "version": manifest.version,
        "description": manifest.description,
        "run": manifest.run,
        "metadata": manifest.metadata,
        "extract": manifest.extract,
        "capabilities": manifest
            .capabilities
            .iter()
            .map(|offered| {
                let mut value = capability(offered);
                value["name"] = json!(offered.name);
                value
            })
            .collect::<Vec<_>>(),
    })
}

fn capability(capability: &Capability) -> Value {
    json!({
        "description": capability.description,
        "input_schema": capability.input.document(),
        "output_schema": capability.output.document(),
        "metadata": capability.metadata,
    })
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
/// stderr follows the error line.
pub(crate) fn call_error(format: Format, error: &CallFailed) -> Output {
    match format {
        Format::Text => {
            let mut stderr = format!("error: {}: {error}\n", error.code_text()).into_bytes();
            if let Some(tail) = error.stderr().filter(|tail| !tail.is_empty()) {
                stderr.extend_from_slice(tail);
                if !tail.ends_with(b"\n") {
                    stderr.push(b'\n');
                }
            }
            Output {
                stderr,
                code: 1,
                ..Output::default()
            }
        }
        Format::Json => document(
            &json!({
                "$type": "board/error/v1",
                "code": error.code_text(),
                "message": error.to_string(),
                "fields": error.fields(),
            }),
            1,
        ),
    }
}
