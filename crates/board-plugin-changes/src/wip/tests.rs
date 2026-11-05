//! The pure rules of the WIP limit: the active window, the stored form of
//! an origin, and the day of a time. The limit itself runs through the
//! binary in board-cli's changes tests.

use board_repos::Date;
use serde_json::Value;

use super::{day_of, split, stored};

fn day(text: &str) -> Date {
    Date::parse(text).unwrap()
}

#[test]
fn split_counts_a_change_active_on_any_of_the_last_n_days_today_included_and_none_without_a_day() {
    let newest = vec![
        ("/a".to_owned(), Some(day("2026-11-02"))),
        ("/b".to_owned(), Some(day("2026-11-01"))),
        ("/c".to_owned(), None),
        ("/d".to_owned(), Some(day("2026-11-04"))),
    ];
    assert_eq!(
        split(&newest, day("2026-11-04"), 3),
        (
            vec!["/a".to_owned(), "/d".to_owned()],
            vec!["/b".to_owned(), "/c".to_owned()]
        )
    );
}

#[test]
fn each_way_of_writing_a_github_origin_has_one_stored_form() {
    for origin in [
        "https://github.com/o/r.git",
        "git@github.com:o/r.git",
        "git@GitHub.com:o/r.git",
        "git@www.github.com:o/r",
        "HTTPS://github.com/o/r",
        "https://www.github.com/O/R",
        "ssh://git@github.com:22/o/r.git",
        "git+ssh://git@github.com/o/r",
        "SSH://git@GITHUB.COM/o/r.git",
    ] {
        assert_eq!(stored(origin), "o/r", "{origin}");
    }
}

#[test]
fn an_origin_off_github_keeps_its_written_form() {
    assert_eq!(
        stored("https://gitlab.com/o/r.git"),
        "https://gitlab.com/o/r.git"
    );
}

#[test]
fn a_time_is_read_as_its_utc_day_and_text_that_is_no_time_has_none() {
    let of = |text: &str| day_of(Some(&Value::from(text)));
    assert_eq!(of("2026-11-04"), Some(day("2026-11-04")));
    assert_eq!(of("2026-11-04T23:30:00Z"), Some(day("2026-11-04")));
    assert_eq!(
        of("2026-11-04T23:30:00.123456-02:00"),
        Some(day("2026-11-05"))
    );
    assert_eq!(of("2026-11-04T00:30:00+01:00"), Some(day("2026-11-03")));
    assert_eq!(of("2026-11-04 10:00"), Some(day("2026-11-04")));
    for wrong in [
        "",
        "yesterday",
        "2026-13-01",
        "2026-11-04T25:00:00Z",
        "2026-11-04T10",
    ] {
        assert_eq!(of(wrong), None, "{wrong}");
    }
    assert_eq!(day_of(Some(&Value::from(3))), None);
    assert_eq!(day_of(None), None);
}
