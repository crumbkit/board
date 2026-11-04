//! What `Host` gives every implementor: `run` is `run_hiding` hiding
//! nothing, `run_hiding` drops only the variables it names, and a board's
//! `places` names each mount of a directory or refuses one it cannot name.

use std::cell::RefCell;
use std::path::PathBuf;

use board_model::ObjectId;
use board_tree::{Matched, Place};

use super::*;

/// A host whose `run_hiding` records what it was asked to hide and answers
/// without starting anything.
struct Recording {
    settings: Settings,
    hidden: RefCell<Vec<Vec<String>>>,
}

impl Host for Recording {
    fn query(&self, _: &str, _: Limit) -> Result<QueryReport, QueryError> {
        unreachable!("this host runs processes only")
    }

    fn refs(
        &self,
        _: &[LogicalPath],
        _: Absent,
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        unreachable!("this host runs processes only")
    }

    fn get_each(&self, _: &[LogicalPath]) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        unreachable!("this host runs processes only")
    }

    fn put(&self, _: &[Value]) -> Result<PutReport, PutError> {
        unreachable!("this host runs processes only")
    }

    fn call(&self, _: &Target, _: &Value) -> Result<CallReport, Box<Refusal>> {
        unreachable!("this host runs processes only")
    }

    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        unreachable!("this host runs processes only")
    }

    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        unreachable!("this host runs processes only")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }

    fn run_hiding(&self, _run: &Run<'_>, hidden: &[&str]) -> Result<Exited, ProcessError> {
        let hidden = hidden.iter().map(|name| (*name).to_owned()).collect();
        self.hidden.borrow_mut().push(hidden);
        Ok(Exited {
            code: Some(3),
            stdout: b"answered".to_vec(),
            stderr: Vec::new(),
        })
    }
}

fn settings(variables: &[(&str, &str)]) -> Settings {
    Settings {
        now: UNIX_EPOCH,
        day_start: None,
        caller: None,
        environment: Variables(
            (variables.iter())
                .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
                .collect(),
        ),
        cache_home: None,
    }
}

/// `sh -c <script>`, run in a new empty directory.
fn shell(arguments: &[String]) -> Run<'_> {
    Run {
        program: "sh",
        arguments,
        directory: None,
        input: b"",
        deadline: Duration::from_secs(5),
    }
}

#[test]
fn run_is_run_hiding_with_nothing_hidden_so_an_override_of_run_hiding_serves_both() {
    let host = Recording {
        settings: settings(&[]),
        hidden: RefCell::new(Vec::new()),
    };
    let arguments = ["-c".to_owned(), "exit 0".to_owned()];
    let ran = host.run(&shell(&arguments)).unwrap();
    assert_eq!((ran.code, ran.stdout), (Some(3), b"answered".to_vec()));
    host.run_hiding(&shell(&arguments), &["SECRET"]).unwrap();
    assert_eq!(
        host.hidden.into_inner(),
        [Vec::<String>::new(), vec!["SECRET".to_owned()]]
    );
}

#[cfg(unix)]
#[test]
fn run_hiding_drops_only_the_variables_it_names_and_run_keeps_them_all() {
    let scenario = super::board(&[("fixture", true)], &["fixture"], Holds::Same);
    let mut board = open(&scenario);
    board.build_in(built(vec![hiding()]));
    let set = Plugins {
        environment: Environment {
            path: Some(OsString::from("/usr/bin:/bin")),
            inherited: [("FIXTURE_GIVEN", "given")]
                .into_iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect(),
            ..Environment::default()
        },
        ..Plugins::default()
    };
    let output = call_base(&board, &set).unwrap().output;
    assert_eq!(
        (&output["all"], &output["none"], &output["hidden"]),
        (
            &json!("given-fixture"),
            &json!("given-fixture"),
            &json!("-fixture")
        )
    );
}

/// `fixture` answering with what `sh` prints of `FIXTURE_GIVEN` and
/// `BOARD_PLUGIN` when run, and when run hiding `FIXTURE_GIVEN`.
fn hiding() -> Fixture {
    answering(
        "fixture",
        Box::new(|host| {
            let arguments = [
                "-c".to_owned(),
                "printf '%s' \"$FIXTURE_GIVEN-$BOARD_PLUGIN\"".to_owned(),
            ];
            let printed = |exited: Exited| String::from_utf8(exited.stdout).unwrap();
            Ok(json!({
                "count": 0,
                "all": printed(host.run(&shell(&arguments)).unwrap()),
                "none": printed(host.run_hiding(&shell(&arguments), &[]).unwrap()),
                "hidden": printed(
                    host.run_hiding(&shell(&arguments), &["FIXTURE_GIVEN", "NOT_SET"]).unwrap()
                ),
            }))
        }),
    )
}

/// `fixture` answering with the logical path of each place `places` gives
/// for `directory`, or with its refusal's code.
fn placing(directory: PathBuf) -> Fixture {
    answering(
        "fixture",
        Box::new(move |host| {
            let places = match host.places(&directory) {
                Ok(places) => json!(places.iter().map(LogicalPath::as_str).collect::<Vec<_>>()),
                Err(refusal) => json!(refusal.code.as_str()),
            };
            Ok(json!({"count": 0, "places": places}))
        }),
    )
}

/// A committed repository, and the commit.
fn library() -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let commit = library.commit(&[("notes/a.md", "a\n")]);
    (library, commit)
}

#[test]
fn a_boards_places_name_each_mount_of_a_directory_and_none_of_another() {
    let (library, commit) = library();
    let other = Scenario::new();
    let scenario = super::board(&[("fixture", true)], &["fixture"], Holds::Same);
    let mount = json!({
        "$type": "board/mount/v1",
        "repo": library.path(),
        "follow": "main",
        "commit": commit.to_string(),
    });
    scenario.write("vendor/lib.json", Item::File(&mount.to_string()));
    for (directory, places) in [
        (library.workdir().to_path_buf(), json!(["/vendor/lib"])),
        (other.workdir().to_path_buf(), json!([])),
    ] {
        let mut board = open(&scenario);
        board.build_in(built(vec![placing(directory)]));
        let output = call_base(&board, &Plugins::default()).unwrap().output;
        assert_eq!(output["places"], places);
    }
}

#[test]
fn a_place_too_long_to_be_a_logical_path_is_refused_and_its_shorter_twin_is_named() {
    let long = |segments: usize| -> LogicalPath {
        format!("/{}", vec!["a".repeat(200); segments].join("/"))
            .parse()
            .unwrap()
    };
    let place = |within: LogicalPath| Place {
        mount: long(10),
        within,
        matched: Matched::Mount,
    };
    let short = place("/notes".parse().unwrap());
    let refused = prefixes(&[short.clone(), place(long(11))]).unwrap_err();
    assert_eq!(refused.code.as_str(), "board::path::malformed");
    let named = prefixes(&[short]).unwrap();
    assert_eq!(named, [format!("{}/notes", long(10)).parse().unwrap()]);
}
