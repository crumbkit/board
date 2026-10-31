//! `put --new`: a record named for the put's UTC minute and a random suffix,
//! drawn again when the name is taken.

use board_model::{Code, Lined};
use board_tree::LOCK_WAIT;

use super::{board, file, path};
use crate::put::PutError;
use crate::put::new::{REDRAWS, drawn, folder_text, name, under};
use crate::tests::{open, task};

/// Draws each of `names` in turn, then the last one again.
fn fixed(names: &[&str]) -> impl FnMut() -> String {
    let mut names: Vec<String> = names.iter().rev().map(|name| (*name).to_owned()).collect();
    move || {
        if names.len() > 1 {
            names.pop().unwrap()
        } else {
            names[0].clone()
        }
    }
}

#[test]
fn a_name_is_the_utc_minute_then_four_lowercase_hex_digits() {
    let cases = [
        (0, 0, "19700101T0000Z-0000"),
        (951_782_400, 0xc2e1, "20000229T0000Z-c2e1"),
        (1_709_164_800, 0xffff, "20240229T0000Z-ffff"),
        (1_793_376_359, 0x0b7c, "20261030T1605Z-0b7c"),
        (4_107_542_399, 1, "21000228T2359Z-0001"),
        (4_107_542_400, 1, "21000301T0000Z-0001"),
    ];
    for (seconds, suffix, expected) in cases {
        assert_eq!(name(seconds, suffix), expected, "{seconds}");
    }
}

#[test]
fn names_drawn_in_one_put_share_its_minute_and_differ_in_their_suffix() {
    let mut draw = drawn();
    let names: Vec<String> = (0..8).map(|_| draw()).collect();
    for drawn in &names {
        assert_eq!(drawn.len(), "20261030T1405Z-c2e1".len(), "{drawn}");
        assert_eq!(&drawn[..14], &names[0][..14], "{drawn}");
        assert!(
            drawn[15..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
    }
    let distinct: std::collections::HashSet<_> = names.iter().collect();
    assert!(distinct.len() > 1, "{names:?}");
}

#[test]
fn a_folder_is_given_with_one_trailing_slash() {
    assert_eq!(folder_text("/tasks/"), "/tasks");
    assert_eq!(folder_text("/"), "/");
    assert_eq!(folder_text("/tasks"), "/tasks");
}

#[test]
fn a_new_record_is_written_under_its_folder_at_the_name_drawn() {
    let scenario = board();
    let report = open(&scenario)
        .put_named(
            &path("/tasks"),
            task("").as_bytes(),
            LOCK_WAIT,
            &mut fixed(&["20261030T1405Z-c2e1"]),
        )
        .unwrap();
    assert_eq!(
        report.writes[0].display.to_string(),
        "/tasks/20261030T1405Z-c2e1"
    );
    assert_eq!(report.writes[0].previous, None);
    assert!(file(&scenario, "tasks/20261030T1405Z-c2e1.json").is_some());
}

#[test]
fn a_taken_name_is_drawn_again_and_every_name_taken_is_refused() {
    let scenario = board();
    let board = open(&scenario);
    let put = |names: &[&str]| {
        board.put_named(
            &path("/tasks"),
            task("").as_bytes(),
            LOCK_WAIT,
            &mut fixed(names),
        )
    };
    put(&["a"]).unwrap();
    let redrawn = put(&["a", "a", "a", "b"]).unwrap();
    assert_eq!(redrawn.writes[0].display.to_string(), "/tasks/b");
    let before = std::fs::read_dir(scenario.workdir().join("tasks"))
        .unwrap()
        .count();
    let refused = put(&["a", "b"]).unwrap_err();
    assert_eq!(refused.code(), Code::PutNameTaken);
    assert_eq!(
        refused.to_string(),
        format!(
            "every name drawn under /tasks is taken, after {} tries",
            REDRAWS + 1
        )
    );
    assert_eq!(refused.fields().path, Some(path("/tasks")));
    let after = std::fs::read_dir(scenario.workdir().join("tasks"))
        .unwrap()
        .count();
    assert_eq!(after, before, "a refused put writes nothing");
}

/// The refusals of a refused batch, as (line, code).
fn refusals(error: PutError) -> (Code, Vec<(usize, Code)>) {
    match error {
        PutError::Batch { code, refusals } => (
            code,
            (refusals.iter())
                .map(|Lined { line, code, .. }| (*line, *code))
                .collect(),
        ),
        other => panic!("not a batch refusal: {other}"),
    }
}

fn new_line(folder: &str) -> String {
    format!(r#"{{"new": "{folder}", "record": {}}}"#, task("")) + "\n"
}

#[test]
fn a_batch_names_each_new_line_and_reports_the_names_it_chose() {
    let scenario = board();
    let input = new_line("/tasks/") + &new_line("/tasks/");
    let report = open(&scenario)
        .put_batch_named(
            &mut input.as_bytes(),
            LOCK_WAIT,
            &mut fixed(&["a", "a", "b"]),
        )
        .unwrap();
    let displays: Vec<String> = (report.writes.iter())
        .map(|written| written.display.to_string())
        .collect();
    assert_eq!(displays, ["/tasks/a", "/tasks/b"]);
}

#[test]
fn a_batch_draws_a_taken_name_again_and_refuses_a_line_whose_every_name_is_taken() {
    let scenario = board();
    let board = open(&scenario);
    let batch = |input: &str, names: &[&str]| {
        board.put_batch_named(&mut input.as_bytes(), LOCK_WAIT, &mut fixed(names))
    };
    batch(&new_line("/tasks/"), &["a"]).unwrap();
    let redrawn = batch(&new_line("/tasks/"), &["a", "b"]).unwrap();
    assert_eq!(redrawn.writes[0].display.to_string(), "/tasks/b");
    let explicit = format!(r#"{{"path": "/tasks/x", "record": {}}}"#, task("")) + "\n";
    let refused = batch(&(explicit + &new_line("/tasks/")), &["a"]).unwrap_err();
    assert_eq!(
        refusals(refused),
        (Code::PutInvalid, vec![(2, Code::PutNameTaken)])
    );
    assert!(file(&scenario, "tasks/x.json").is_none());
}

#[test]
fn a_new_line_whose_folder_lacks_its_slash_is_refused_and_its_twin_is_admitted() {
    let scenario = board();
    let board = open(&scenario);
    let batch =
        |input: &str| board.put_batch_named(&mut input.as_bytes(), LOCK_WAIT, &mut fixed(&["a"]));
    let refused = batch(&(new_line("/tasks/") + &new_line("/tasks")));
    assert_eq!(
        refusals(refused.unwrap_err()),
        (Code::PutInvalid, vec![(2, Code::PutNotAFolder)])
    );
    let admitted = batch(&new_line("/tasks/")).unwrap();
    assert_eq!(admitted.writes[0].display.to_string(), "/tasks/a");
}

#[test]
fn a_new_line_with_a_path_an_expect_or_no_record_is_refused_and_its_twin_is_admitted() {
    let scenario = board();
    let board = open(&scenario);
    let record = task("");
    for refused in [
        format!(r#"{{"new": "/tasks/", "path": "/tasks/a", "record": {record}}}"#),
        format!(r#"{{"new": "/tasks/", "expect": "none", "record": {record}}}"#),
        r#"{"new": "/tasks/", "text": "a"}"#.to_owned(),
        format!(r#"{{"record": {record}}}"#),
    ] {
        let error = board
            .put_batch_named(
                &mut format!("{refused}\n").as_bytes(),
                LOCK_WAIT,
                &mut fixed(&["a"]),
            )
            .unwrap_err();
        assert_eq!(
            refusals(error),
            (Code::PutInvalid, vec![(1, Code::PutInvalid)]),
            "{refused}"
        );
    }
    let admitted = format!(r#"{{"new": "/tasks/", "record": {record}}}"#) + "\n";
    board
        .put_batch_named(&mut admitted.as_bytes(), LOCK_WAIT, &mut fixed(&["a"]))
        .unwrap();
}

#[test]
fn a_new_line_whose_name_is_taken_beside_another_stale_line_is_refused_as_taken_and_its_twin_is_admitted()
 {
    let scenario = board();
    let board = open(&scenario);
    let batch = |input: &str, names: &[&str]| {
        board.put_batch_named(&mut input.as_bytes(), LOCK_WAIT, &mut fixed(names))
    };
    batch(&(new_line("/tasks/")), &["a"]).unwrap();
    batch(&(new_line("/tasks/")), &["x"]).unwrap();
    let explicit =
        |expect: &str| format!(r#"{{"path": "/tasks/x", {expect}"record": {}}}"#, task("")) + "\n";
    let refused = batch(
        &(explicit(r#""expect": "none", "#) + &new_line("/tasks/")),
        &["a"],
    );
    assert_eq!(
        refusals(refused.unwrap_err()),
        (
            Code::PutInvalid,
            vec![(1, Code::PutStale), (2, Code::PutNameTaken)]
        )
    );
    let admitted = batch(&(explicit("") + &new_line("/tasks/")), &["a", "b"]).unwrap();
    assert_eq!(admitted.writes[1].display.to_string(), "/tasks/b");
}

#[test]
fn a_name_that_takes_the_path_past_its_longest_is_refused_and_one_that_fits_is_admitted() {
    let name = "20261030T1405Z-c2e1";
    // A folder of `bytes` bytes, in segments of at most 200.
    let folder = |bytes: usize| {
        let mut text = String::new();
        while text.len() < bytes {
            let segment = (bytes - text.len() - 1).min(199);
            text.push('/');
            text.push_str(&"a".repeat(segment));
        }
        path(&text)
    };
    let fits = board_model::MAX_PATH_BYTES - name.len() - 1;
    let refused = under(&folder(fits + 1), &mut || name.to_owned()).unwrap_err();
    assert_eq!(refused.code(), Code::PathMalformed);
    let hint = refused.refusal().hint.unwrap();
    assert!(hint.contains("would be over 4096 bytes"), "{hint}");
    assert_eq!(
        refused.fields().directory.as_deref(),
        Some(&folder(fits + 1).as_str()[1..])
    );
    let admitted = under(&folder(fits), &mut || name.to_owned()).unwrap();
    assert_eq!(admitted.as_str().len(), board_model::MAX_PATH_BYTES);

    let scenario = board();
    let batch = |folder: &str| {
        let input = new_line(&format!("{folder}/"));
        open(&scenario).put_batch_named(&mut input.as_bytes(), LOCK_WAIT, &mut fixed(&[name]))
    };
    let refused = batch(folder(fits + 1).as_str()).unwrap_err();
    assert_eq!(
        refusals(refused),
        (Code::PutInvalid, vec![(1, Code::PathMalformed)])
    );
    batch("/tasks").unwrap();
}
