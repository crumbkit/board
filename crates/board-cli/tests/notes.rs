//! The local time zone reaches a built-in plugin through the real `board`
//! binary: board-cli's config module reads `TZ` with jiff, and notes
//! dates the day by the UTC offset Board passes it. The in-process suite
//! passes the offset itself, so only this test reads `TZ`.
#![cfg(test)]
#![expect(
    clippy::disallowed_methods,
    reason = "the test runs the real board with TZ set, as a person's shell does"
)]

use std::path::Path;
use std::process::Command;

use board_scenario::{Item, Scenario};
use serde_json::Value;

/// `board --json call notes.<capability> --repo /` in `board`, with `TZ`
/// set to `zone`: its output.
fn call(board: &Scenario, capability: &str, flags: &[&str], zone: &str) -> Value {
    let xdg = Scenario::new();
    let binary = Path::new(env!("CARGO_BIN_EXE_board"));
    let target = format!("notes.{capability}");
    let ran = Command::new(binary)
        .args(["--json", "call", &target, "--repo", "/"])
        .args(flags)
        .current_dir(board.workdir())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TZ", zone)
        .env("XDG_DATA_HOME", xdg.workdir().join("data"))
        .env("XDG_CACHE_HOME", xdg.workdir().join("cache"))
        .env("XDG_CONFIG_HOME", xdg.workdir().join("config"))
        .output()
        .unwrap();
    let document: Value = serde_json::from_slice(&ran.stdout).unwrap();
    assert!(ran.status.success(), "{document}");
    document["data"]["output"].clone()
}

/// The daily note's path and the `## HH:` heading in `zone` now.
fn local(zone: &str) -> (String, String) {
    let now = jiff::Timestamp::now().to_zoned(jiff::tz::TimeZone::get(zone).unwrap());
    (
        format!("/daily/{}.md", now.date()),
        format!("## {:02}:", now.hour()),
    )
}

#[test]
fn notes_dates_the_day_and_the_heading_by_the_callers_tz() {
    let board = Scenario::new();
    board
        .write(
            ".board/board.json",
            Item::File(
                r#"{"$type": "board/config/v1", "commit_on_put": false, "plugins": ["notes"]}"#,
            ),
        )
        .write(
            ".board/types/notes/repo/v1.schema.json",
            Item::File(include_str!(
                "../../board-plugin-notes/types/notes/repo/v1.schema.json"
            )),
        )
        .write("daily/.gitkeep", Item::File(""));
    // Kiritimati is 14 hours ahead of UTC and Etc/GMT+12 is 12 behind, so
    // their dates always differ.
    let mut paths = Vec::new();
    for zone in ["Pacific/Kiritimati", "Etc/GMT+12"] {
        let before = local(zone);
        let daily = call(&board, "daily", &[], zone);
        let added = call(&board, "add", &["--text", "Here."], zone);
        let after = local(zone);
        let path = added["path"].as_str().unwrap();
        assert_eq!(daily["path"], path, "{zone}");
        assert!(
            [&before.0, &after.0].contains(&&path.to_owned()),
            "{zone}: {path}"
        );
        let note = std::fs::read_to_string(board.workdir().join(&path[1..])).unwrap();
        let at = note.rfind("## ").unwrap();
        let heading = note[at..at + 6].to_owned();
        assert!([&before.1, &after.1].contains(&&heading), "{zone}: {note}");
        paths.push(path.to_owned());
    }
    assert_ne!(paths[0], paths[1], "the two zones are on different days");
}
