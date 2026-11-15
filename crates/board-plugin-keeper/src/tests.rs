//! The manifest parses as Board reads it, and each part of the plugin has
//! its own module of tests.

use board_model::RecordType;
use board_ops::Plugin;
use serde_json::{Value, json};

use super::*;

mod record;

/// A synthetic map with two realms, two projects, a finding, a system with a
/// link, and question rules.
pub(crate) fn sample_map() -> Value {
    json!({
        "$type": "keeper/map/v1",
        "date": "2026-02-03",
        "source": "/projects/demo/curation/2026-02-03.md",
        "realms": [{"id": "alpha", "title": "Alpha"}, {"id": "beta", "title": "Beta"}],
        "projects": [{
            "id": "speed",
            "realm": "alpha",
            "title": "Make the parser fast",
            "state": "active",
            "summary": "The cache lands next.",
            "task": null,
            "changes": ["/changes/cache", "/changes/bench"],
            "tasks": ["/tasks/profile.md"],
            "order": [["/changes/bench", "/changes/cache"]],
            "questions": [],
        }, {
            "id": "docs",
            "realm": "beta",
            "title": "Write the guide",
            "state": "waiting-on-owner",
            "task": "/tasks/guide.md",
            "changes": ["/changes/guide"],
        }],
        "findings": [{
            "kind": "stale",
            "paths": ["/changes/guide"],
            "evidence": "No commit for 9 days.",
            "fix": "changes.pause",
            "id": "stale-0a1b2c3d",
        }],
        "questions_waiting": {"alpha": 1, "beta": 0},
        "dedupe": {"/repos/beta/repos/notes": "/repos/notes"},
        "systems": [{
            "id": "parser",
            "realm": "alpha",
            "title": "Parser",
            "language": "Rust",
            "crates_from": "/repos/parser",
            "modules": [{"id": "lexer", "lines": 1200, "purpose": "Turns text into tokens."}],
            "links": [{
                "id": "parser-to-store",
                "name": "Store API",
                "module": "lexer",
                "to": "store.api",
                "kind": "api-call",
                "traffic": 4,
                "production": true,
                "controls": [{"caption": "Token gate", "detail": "Needs a token.", "at": "to"}],
            }],
        }, {"id": "store", "realm": "beta", "title": "Store", "external": true}],
        "question_rules": {
            "mount_realms": {"team": "beta"},
            "localRealm": {"notes": "alpha"},
            "localDefault": "alpha",
            "tasks": [{"contains": "profile", "to": "speed"}],
            "names": {"guide": "docs", "team/guide": "docs"},
        },
    })
}

#[test]
fn the_manifest_parses_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Keeper.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("keeper", "0.1.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, ["keeper/map/v1"]);
    assert_eq!(manifest.description, skill_description());
}
