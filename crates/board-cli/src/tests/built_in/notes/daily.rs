//! `notes.daily`, and the person's day across a UTC offset. These were
//! `test_daily.py` and the day tests of `test_repos.py`.

use super::*;

#[test]
fn the_daily_note_the_phone_pushed_is_read_after_a_sync() {
    let notes = Notes::new();
    notes.phone_appends("daily/2026-01-05.md", "From the phone.\n");
    let output = notes.call("daily", &["--date", "2026-01-05"]).unwrap();
    let head = git(
        &notes.beside.path().join("remote.git"),
        &["rev-parse", "main"],
    );
    assert_eq!(
        output,
        json!({
            "repo": "/repos/notes-personal",
            "path": "/repos/notes-personal/daily/2026-01-05.md",
            "exists": true,
            "private": false,
            "text": "From the phone.\n",
            "commit": head,
            "unsynced": null,
        })
    );
    assert_eq!(notes.pin(), head);
}

#[test]
fn the_day_read_by_default_is_today_in_the_callers_time_zone() {
    let notes = Notes::new();
    for zone in ZONES {
        let output = notes.call_in("daily", &[], zone, None).unwrap();
        let expected: Vec<String> = (dates(zone, 0).iter())
            .map(|date| format!("/repos/notes-personal/daily/{date}.md"))
            .collect();
        assert!(
            expected.contains(&output["path"].as_str().unwrap().to_owned()),
            "{output}"
        );
    }
}

#[test]
fn a_day_without_a_note_reads_as_none_and_creates_no_note() {
    let notes = Notes::new();
    let before = notes.head();
    let output = notes.call("daily", &["--date", "2026-01-02"]).unwrap();
    assert_eq!(
        (&output["exists"], &output["text"]),
        (&json!(false), &Value::Null)
    );
    assert!(!notes.clone.workdir().join("daily/2026-01-02.md").exists());
    assert_eq!(notes.head(), before);
}

#[test]
fn a_private_daily_note_is_not_returned_and_one_that_is_not_private_is() {
    let notes = Notes::new();
    notes.phone_appends("daily/2026-01-02.md", "---\nprivate: true\n---\nSecret.\n");
    notes.phone_appends("daily/2026-01-03.md", "---\nprivate: false\n---\nOpen.\n");
    let hidden = notes.call("daily", &["--date", "2026-01-02"]).unwrap();
    assert_eq!(
        (&hidden["exists"], &hidden["private"], &hidden["text"]),
        (&json!(true), &json!(true), &Value::Null)
    );
    let shown = notes.call("daily", &["--date", "2026-01-03"]).unwrap();
    assert_eq!(
        (&shown["exists"], &shown["private"], &shown["text"]),
        (
            &json!(true),
            &json!(false),
            &json!("---\nprivate: false\n---\nOpen.\n")
        )
    );
}

#[test]
fn a_date_that_is_not_a_day_is_refused_and_one_that_is_is_read() {
    let notes = Notes::new();
    let refused = notes.refused("daily", &["--date", "2026-02-30"]);
    assert_eq!(refused["code"], "board::plugin::bad_input");
    notes.call("daily", &["--date", "2026-02-28"]).unwrap();
}

/// The zone whose local time is now `minutes` past midnight, and the UTC
/// date and the local date there. The offset is under a day either way,
/// so the two dates differ unless the local time is UTC's.
fn zone_at(minutes: i64) -> (Zone, String, String) {
    let now = jiff::Timestamp::now();
    let utc = now.to_zoned(jiff::tz::TimeZone::UTC);
    let into =
        i64::from(utc.hour()) * 3600 + i64::from(utc.minute()) * 60 + i64::from(utc.second());
    let mut offset = minutes * 60 - into;
    if offset > 14 * 3600 {
        offset -= 86_400;
    }
    if offset < -12 * 3600 {
        offset += 86_400;
    }
    let zone = Zone(i32::try_from(offset).unwrap());
    let local = now.to_zoned(
        jiff::tz::Offset::from_seconds(zone.0)
            .unwrap()
            .to_time_zone(),
    );
    (zone, utc.date().to_string(), local.date().to_string())
}

/// The UTC date and the date `zone` is on now.
fn dates_now(zone: Zone) -> (String, String) {
    let now = jiff::Timestamp::now();
    let local = now.to_zoned(
        jiff::tz::Offset::from_seconds(zone.0)
            .unwrap()
            .to_time_zone(),
    );
    (
        now.to_zoned(jiff::tz::TimeZone::UTC).date().to_string(),
        local.date().to_string(),
    )
}

#[test]
fn daily_reads_the_note_of_the_local_day_across_a_utc_offset_and_utc_reads_utcs() {
    // Before UTC's noon, 12 hours behind is yesterday; from noon on, 14
    // hours ahead is tomorrow.
    let utc_hour = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .hour();
    let zone = if utc_hour < 12 {
        Zone(-12 * 3600)
    } else {
        Zone(14 * 3600)
    };
    let notes = Notes::new();
    let before = dates_now(zone);
    let read = notes.call_in("daily", &[], zone, None).unwrap();
    let utc = notes.call_in("daily", &[], UTC, None).unwrap();
    let after = dates_now(zone);
    let path = |date: &str| json!(format!("/repos/notes-personal/daily/{date}.md"));
    assert!(
        [path(&before.1), path(&after.1)].contains(&read["path"]),
        "{read}"
    );
    assert!(
        [path(&before.0), path(&after.0)].contains(&utc["path"]),
        "{utc}"
    );
    assert_ne!(read["path"], utc["path"], "the local day is not UTC's");
}

#[test]
fn a_note_at_one_in_the_morning_lands_in_yesterdays_daily_note_when_the_day_starts_at_seven() {
    let notes = Notes::new();
    let (zone, _, local) = zone_at(60 + 15);
    let today: jiff::civil::Date = local.parse().unwrap();
    let yesterday = today.yesterday().unwrap().to_string();
    notes.day_start(Some("07:00"));
    let output = notes
        .call_in("add", &["--text", "Late."], zone, None)
        .unwrap();
    assert_eq!(Notes::note(&output), format!("daily/{yesterday}.md"));
    let text = notes.read(&Notes::note(&output));
    assert!(
        text.starts_with("## 01:") && text.ends_with("\n\nLate.\n"),
        "{text}"
    );
    let read = notes.call_in("daily", &[], zone, None).unwrap();
    assert_eq!(read["path"], output["path"], "daily reads the same day");
    notes.day_start(None);
    let output = notes
        .call_in("add", &["--text", "Midnight."], zone, None)
        .unwrap();
    assert_eq!(
        Notes::note(&output),
        format!("daily/{local}.md"),
        "without day_start, today"
    );
}

#[test]
fn a_day_start_that_is_not_hh_mm_is_refused_by_board_and_one_that_is_is_admitted() {
    let notes = Notes::new();
    notes.day_start(Some("7am"));
    let refused = notes.refused("add", &["--text", "When?"]);
    assert_eq!(refused["code"], "board::config::invalid");
    notes.day_start(Some("07:00"));
    notes.call("add", &["--text", "When?"]).unwrap();
}
