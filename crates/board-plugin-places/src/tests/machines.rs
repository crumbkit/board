//! The machine label: each source wins over the later ones, a blank source
//! gives way to the next, the name becomes a slug as the runbook script
//! makes one, and a name that makes no slug is `places::machine::unnamed`.

use board_scenario::{Item, Scenario};
use proptest::prelude::*;

use super::host::{Answer, Real};
use crate::machine::{label, read};

fn named(sources: [Option<&str>; 4]) -> Option<String> {
    label(sources.map(|source| source.map(str::to_owned)))
}

#[test]
fn each_source_wins_over_every_later_one() {
    let all = [Some("one"), Some("two"), Some("three"), Some("four")];
    assert_eq!(named(all).as_deref(), Some("one"));
    assert_eq!(
        named([None, Some("two"), Some("three"), Some("four")]).as_deref(),
        Some("two")
    );
    assert_eq!(
        named([None, None, Some("three"), Some("four")]).as_deref(),
        Some("three")
    );
    assert_eq!(
        named([None, None, None, Some("four")]).as_deref(),
        Some("four")
    );
    assert_eq!(named([None, None, None, None]), None);
}

#[test]
fn a_blank_source_gives_way_to_the_next_and_a_named_one_does_not() {
    assert_eq!(
        named([Some("  \n"), Some("two"), None, None]).as_deref(),
        Some("two")
    );
    assert_eq!(
        named([Some(" one "), Some("two"), None, None]).as_deref(),
        Some("one")
    );
}

#[test]
fn a_name_is_lower_cased_and_each_run_of_other_characters_becomes_one_hyphen() {
    for (name, slug) in [
        ("Quokka-Mini", "quokka-mini"),
        ("Quokka's MacBook Pro", "quokka-s-macbook-pro"),
        ("--quokka__2--", "quokka-2"),
        ("quokka.local", "quokka-local"),
        ("Café", "caf"),
    ] {
        assert_eq!(
            named([Some(name), None, None, None]).as_deref(),
            Some(slug),
            "{name}"
        );
    }
}

#[test]
fn a_name_of_only_other_characters_is_unnamed_even_when_a_later_source_names_one_and_a_letter_is_not()
 {
    assert_eq!(named([Some("--__--"), Some("two"), None, None]), None);
    assert_eq!(
        named([Some("-a-"), Some("two"), None, None]).as_deref(),
        Some("a")
    );
}

proptest! {
    #[test]
    fn any_sources_give_a_label_that_matches_the_pattern_or_none(
        sources in proptest::array::uniform4(proptest::option::of(".{0,12}"))
    ) {
        if let Some(label) = label(sources) {
            let bytes = label.as_bytes();
            prop_assert!(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit(), "{label}");
            prop_assert!(
                bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-'),
                "{label}"
            );
        }
    }
}

#[test]
fn the_variable_is_read_first_and_runs_no_process() {
    let host = Real::new().environment(&[("BOARD_MACHINE", "Quokka Mini")]);
    host.answer("scutil --get LocalHostName", Answer::printing("other\n"));
    assert_eq!(read(&host).unwrap(), "quokka-mini");
    assert_eq!(host.runs.borrow().len(), 0);
}

#[test]
fn the_files_first_line_is_read_when_the_variable_is_unset_or_blank() {
    let home = Scenario::new();
    home.write(".config/board/machine", Item::File("wombat\nignored\n"));
    let path = home.workdir().to_str().unwrap().to_owned();
    for variables in [
        vec![("HOME", path.as_str())],
        vec![("HOME", path.as_str()), ("BOARD_MACHINE", " ")],
    ] {
        let host = Real::new().environment(&variables);
        host.answer("scutil --get LocalHostName", Answer::printing("other\n"));
        assert_eq!(read(&host).unwrap(), "wombat");
        assert_eq!(host.runs.borrow().len(), 0);
    }
}

#[test]
fn scutil_names_the_machine_when_no_file_does_and_hostname_when_scutil_fails() {
    let home = Scenario::new();
    let path = home.workdir().to_str().unwrap().to_owned();
    let host = Real::new().environment(&[("HOME", path.as_str())]);
    host.answer(
        "scutil --get LocalHostName",
        Answer::printing("Numbat-Air\n"),
    );
    host.answer("hostname -s", Answer::printing("other\n"));
    assert_eq!(read(&host).unwrap(), "numbat-air");
    assert_eq!(*host.runs.borrow(), ["scutil --get LocalHostName"]);

    let host = Real::new().environment(&[("HOME", path.as_str())]);
    host.answer(
        "scutil --get LocalHostName",
        Answer::Exits {
            code: 1,
            stdout: "Numbat-Air\n".to_owned(),
            stderr: String::new(),
        },
    );
    host.answer("hostname -s", Answer::printing("bilby\n"));
    assert_eq!(read(&host).unwrap(), "bilby");
}

#[test]
fn hostname_names_the_machine_where_scutil_is_not_on_path() {
    let host = Real::new().environment(&[]);
    host.answer("hostname -s", Answer::printing("bilby\n"));
    assert_eq!(read(&host).unwrap(), "bilby");
    assert_eq!(
        *host.runs.borrow(),
        ["scutil --get LocalHostName", "hostname -s"]
    );
}

#[test]
fn with_no_source_naming_the_machine_it_is_unnamed_and_with_one_it_is_not() {
    let host = Real::new().environment(&[]);
    let refused = read(&host).unwrap_err();
    assert_eq!(refused.code, "places::machine::unnamed");
    assert!(
        refused.message.contains("BOARD_MACHINE"),
        "{}",
        refused.message
    );
    let host = Real::new().environment(&[("BOARD_MACHINE", "bilby")]);
    assert_eq!(read(&host).unwrap(), "bilby");
}
