use super::*;
use crate::tests::Fixture;

fn parse(text: &str) -> Option<Profile> {
    Profile::parse(text.split_whitespace().map(str::to_owned))
}

#[test]
fn profile_reads_board_arguments_after_a_separator_and_an_optional_file_to_save() {
    assert_eq!(
        parse("-- query x"),
        Some(Profile {
            save: None,
            arguments: vec!["query".into(), "x".into()]
        })
    );
    assert_eq!(
        parse("--save p.json -- check").unwrap().save,
        Some("p.json".into())
    );
    assert_eq!(parse("--save p.json check"), None);
    assert_eq!(parse("check"), None);
    assert_eq!(parse("--"), None);
}

#[test]
fn samply_missing_from_path_is_refused_and_samply_on_path_is_found() {
    let fixture = Fixture::new();
    fixture.write("bin/samply", "");
    let empty = fixture.path().join("empty");
    let refused = samply(empty.as_os_str()).unwrap_err();
    assert_eq!(refused.code(), "xtask::profile::no_samply");
    assert_eq!(
        refused.to_string(),
        "samply is not on PATH; install it with `cargo install --locked samply`"
    );
    let path = std::env::join_paths([empty, fixture.path().join("bin")]).unwrap();
    assert_eq!(samply(&path).unwrap(), fixture.path().join("bin/samply"));
}

#[test]
fn a_step_that_fails_is_refused_with_why_and_one_that_succeeds_is_admitted() {
    let missing = status(&mut process::command("/nonexistent/samply")).unwrap_err();
    assert_eq!(missing.code(), "xtask::profile::failed");
    assert!(std::error::Error::source(&missing).is_some(), "{missing}");
    let exited = status(process::command("sh").args(["-c", "exit 3"])).unwrap_err();
    assert_eq!(exited.code(), "xtask::profile::failed");
    assert!(
        exited
            .to_string()
            .starts_with("sh stopped with exit status: 3; "),
        "{exited}"
    );
    status(process::command("sh").args(["-c", "exit 0"])).unwrap();
}
