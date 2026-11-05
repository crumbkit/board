//! The sweep's sections at a fixed moment, from tasks as `available` reads
//! them, with no Board. These were the plugin's `test_sweep.py`.

use super::*;
use crate::tree::forest;

/// Three hours east of UTC.
const OFFSET: i32 = 3 * 3600;
const SEVEN: Duration = Duration::from_hours(7);

/// The moment at `date` and `hour` local time, three hours east of UTC.
fn at(date: &str, hour: i64) -> i64 {
    Date::parse(date).unwrap().days() * 86_400 + hour * 3600 - i64::from(OFFSET)
}

/// 25 October 2026, 10:00 local.
fn now() -> i64 {
    at("2026-10-25", 10)
}

fn clock(now: i64, start: Duration) -> Clock {
    let moment = SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(now).unwrap());
    Clock {
        now,
        offset: OFFSET,
        start,
        day: today(moment, OFFSET, start),
    }
}

fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}

/// An open task in the board's own tree, reviewed on the moment's day.
fn task(title: &str) -> Task {
    Task {
        repo: "/".to_owned(),
        path: format!("/tasks/{title}.md"),
        title: title.to_owned(),
        due: None,
        defer: None,
        from_hour: None,
        reviewed: Some(date("2026-10-25")),
        contexts: Vec::new(),
        waits: false,
        owner: None,
        status: "open".to_owned(),
        linked: false,
        kind: "task".to_owned(),
        brief: None,
    }
}

/// The task deferred by `defer`, as the frontmatter writes it.
fn deferred_by(title: &str, defer: &str) -> Task {
    Task {
        defer: Some((defer.to_owned(), Defer::parse(defer, OFFSET).unwrap())),
        ..task(title)
    }
}

/// The sections of the sweep when no task has a parent.
fn swept(
    tasks: &[Task],
    blocked: &BTreeMap<String, Vec<String>>,
    clock: &Clock,
    owner: Option<&str>,
) -> [Section; 4] {
    let (tree, nodes) = (forest(&Nodes::new()), Nodes::new());
    let places = Places {
        tree: &tree,
        nodes: &nodes,
    };
    sweep(tasks, blocked, places, clock, owner)
}

/// The titles in each section: available, due soon, stale and held.
fn sections(
    tasks: &[Task],
    clock: &Clock,
    blocked: &BTreeMap<String, Vec<String>>,
) -> [Vec<String>; 4] {
    swept(tasks, blocked, clock, None)
        .map(|section| section.tasks.into_iter().map(|item| item.title).collect())
}

fn titles(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_task_deferred_to_a_later_date_or_moment_waits_and_one_deferred_to_now_or_earlier_is_available()
{
    let tasks = [
        deferred_by("tomorrow", "2026-10-26"),
        deferred_by("today", "2026-10-25"),
        deferred_by("in-an-hour", "2026-10-25T11:00:00+03:00"),
        deferred_by("an-hour-ago", "2026-10-25T09:00:00+03:00"),
    ];
    let [available, ..] = sections(&tasks, &clock(now(), SEVEN), &BTreeMap::new());
    assert_eq!(available, titles(&["an-hour-ago", "today"]));
}

#[test]
fn a_horizon_counts_from_reviewed_and_without_reviewed_defers_nothing() {
    let tasks = [
        deferred_by("soon-from-today", "soon"),
        Task {
            reviewed: Some(date("2026-10-01")),
            ..deferred_by("soon-from-long-ago", "soon")
        },
        Task {
            reviewed: Some(date("2026-10-24")),
            ..deferred_by("not-today-from-yesterday", "not-today")
        },
        Task {
            reviewed: None,
            ..deferred_by("someday-unreviewed", "someday")
        },
    ];
    let found = sections(&tasks, &clock(now(), SEVEN), &BTreeMap::new());
    let available = titles(&[
        "not-today-from-yesterday",
        "someday-unreviewed",
        "soon-from-long-ago",
    ]);
    assert_eq!(found, [available, Vec::new(), Vec::new(), Vec::new()]);
}

#[test]
fn an_hour_before_the_day_starts_belongs_to_the_day_before() {
    let evening = [
        Task {
            from_hour: Some(17),
            ..task("evening")
        },
        Task {
            from_hour: Some(3),
            ..task("dawn")
        },
    ];
    let none = BTreeMap::new();
    let one_am = at("2026-10-26", 1);
    let [available, ..] = sections(&evening, &clock(one_am, SEVEN), &none);
    assert_eq!(
        available,
        titles(&["evening"]),
        "17:00 on the 25th has passed; 03:00 on the 26th not"
    );
    let [available, ..] = sections(&evening, &clock(now(), SEVEN), &none);
    assert!(
        available.is_empty(),
        "neither 17:00 nor the next 03:00 has come"
    );
    let [available, ..] = sections(&evening, &clock(one_am, Duration::ZERO), &none);
    assert!(
        available.is_empty(),
        "with a midnight start, 01:00 is a new day"
    );
    let [available, ..] = sections(&evening, &clock(at("2026-10-26", 4), SEVEN), &none);
    assert_eq!(available, titles(&["dawn", "evening"]));
}

#[test]
fn a_blocked_task_waits_and_its_blocker_counts_it() {
    let tasks = [
        task("blocker"),
        Task {
            waits: true,
            ..task("waiter")
        },
    ];
    let blocked = BTreeMap::from([(
        "/tasks/waiter.md".to_owned(),
        vec!["/tasks/blocker.md".to_owned()],
    )]);
    let [available, ..] = swept(&tasks, &blocked, &clock(now(), SEVEN), None);
    let shown: Vec<(String, usize, Vec<String>)> = (available.tasks.into_iter())
        .map(|item| (item.title, item.waiting, item.blocked_by))
        .collect();
    assert_eq!(shown, [("blocker".to_owned(), 1, Vec::new())]);
}

#[test]
fn due_soon_holds_every_open_task_due_within_seven_days_or_overdue_whether_available_or_not() {
    let tasks = [
        Task {
            due: Some(date("2026-10-01")),
            ..task("overdue")
        },
        Task {
            due: Some(date("2026-11-01")),
            ..deferred_by("in-seven-days", "someday")
        },
        Task {
            due: Some(date("2026-11-02")),
            ..task("in-eight-days")
        },
    ];
    let [_, due_soon, ..] = sections(&tasks, &clock(now(), SEVEN), &BTreeMap::new());
    assert_eq!(due_soon, titles(&["in-seven-days", "overdue"]));
}

#[test]
fn a_task_not_available_is_stale_without_reviewed_or_reviewed_over_two_weeks_ago_unless_deferred() {
    let old = Some(date("2026-10-10"));
    let waiting = |title: &str, reviewed: Option<Date>| Task {
        reviewed,
        waits: true,
        ..task(title)
    };
    let tasks = [
        waiting("blocked-old", old),
        waiting("blocked-recent", Some(date("2026-10-11"))),
        waiting("blocked-unreviewed", None),
        Task {
            reviewed: old,
            ..deferred_by("deferred-old", "2026-12-01")
        },
        Task {
            reviewed: None,
            ..deferred_by("deferred-unreviewed", "2026-12-01")
        },
        Task {
            reviewed: old,
            ..deferred_by("someday-old", "someday")
        },
        Task {
            reviewed: old,
            ..task("available-old")
        },
    ];
    let blocked: BTreeMap<String, Vec<String>> = (tasks.iter())
        .filter(|item| item.waits)
        .map(|item| (item.path.clone(), vec!["/tasks/x.md".to_owned()]))
        .collect();
    let [_, _, stale, _] = sections(&tasks, &clock(now(), SEVEN), &blocked);
    assert_eq!(stale, titles(&["blocked-old", "blocked-unreviewed"]));
}

#[test]
fn a_section_lists_at_most_a_hundred_tasks_by_repo_then_title_and_counts_them_all() {
    let mut tasks: Vec<Task> = (0..=101)
        .rev()
        .map(|number| task(&format!("t{number:03}")))
        .collect();
    tasks.push(Task {
        repo: "/repos/a".to_owned(),
        path: "/repos/a/tasks/a.md".to_owned(),
        ..task("a")
    });
    let [available, ..] = swept(&tasks, &BTreeMap::new(), &clock(now(), SEVEN), None);
    assert_eq!((available.tasks.len(), available.total), (100, 103));
    let shown: Vec<&str> = available
        .tasks
        .iter()
        .map(|item| item.title.as_str())
        .collect();
    assert_eq!(shown[..3], ["t000", "t001", "t002"]);
    assert_eq!(shown[99], "t099");
    assert!(!shown.contains(&"a"), "/ sorts before /repos/a");
}

#[test]
fn a_date_time_with_a_lowercase_t_or_without_an_offset_is_a_moment_too() {
    let tasks = [
        deferred_by("lower-later", "2026-10-25t11:00:00+03:00"),
        deferred_by("lower-earlier", "2026-10-25t09:00:00+03:00"),
        deferred_by("local-later", "2026-10-25T11:00:00"),
    ];
    let [available, ..] = sections(&tasks, &clock(now(), SEVEN), &BTreeMap::new());
    assert_eq!(available, titles(&["lower-earlier"]));
}

#[test]
fn a_date_defer_ends_when_the_persons_day_starts_and_not_at_midnight() {
    let tasks = [deferred_by("monday", "2026-10-26")];
    let none = BTreeMap::new();
    let [available, ..] = sections(&tasks, &clock(at("2026-10-26", 5), SEVEN), &none);
    assert_eq!(available, Vec::<String>::new());
    let [available, ..] = sections(&tasks, &clock(at("2026-10-26", 8), SEVEN), &none);
    assert_eq!(available, titles(&["monday"]));
}

#[test]
fn a_task_whose_dates_cannot_be_read_is_degraded_and_one_whose_dates_can_is_read() {
    let row = |defer: &str, due: Option<&str>| {
        let mut body = json!({
            "$type": "tasks/task/v2", "title": defer, "status": "open", "defer": defer,
            "reviewed": "2026-10-25",
        });
        if let Some(due) = due {
            body["due"] = json!(due);
        }
        [
            json!(""),
            json!(format!("/tasks/{defer}.md")),
            Value::Null,
            json!(body.to_string()),
        ]
    };
    let rows = [
        row("2026-10-25T25:00:00Z", None),
        row("tomorrow", None),
        row("soon", Some("2026-13-01")),
        row("2026-10-25t10:00:00Z", None),
    ];
    let (tasks, degraded) = crate::listing::read(&rows, &["/".to_owned()], OFFSET);
    let read: Vec<&str> = tasks.iter().map(|item| item.title.as_str()).collect();
    assert_eq!((read, degraded), (vec!["2026-10-25t10:00:00Z"], 3));
}

#[test]
fn an_owner_keeps_the_tasks_it_or_nobody_holds_and_held_lists_its_tasks_that_are_not_available() {
    let due = Some(date("2026-10-26"));
    let owned = |owner: Option<&str>, task: Task| Task {
        owner: owner.map(str::to_owned),
        ..task
    };
    let tasks = [
        owned(
            Some("lead"),
            Task {
                due,
                ..task("mine")
            },
        ),
        Task {
            due,
            ..task("nobodys")
        },
        owned(
            Some("other"),
            Task {
                due,
                ..task("theirs")
            },
        ),
        owned(Some("lead"), deferred_by("mine-deferred", "2026-10-30")),
        owned(
            Some("lead"),
            Task {
                waits: true,
                ..task("mine-blocked")
            },
        ),
        owned(
            Some("lead"),
            Task {
                from_hour: Some(17),
                ..task("mine-later-today")
            },
        ),
        owned(Some("other"), deferred_by("theirs-deferred", "2026-10-30")),
    ];
    let blocked = BTreeMap::from([(
        "/tasks/mine-blocked.md".to_owned(),
        vec!["/tasks/theirs.md".to_owned()],
    )]);
    let clock = clock(now(), SEVEN);
    let found = swept(&tasks, &blocked, &clock, Some("lead"));
    let names = |section: &Section| -> Vec<String> {
        section
            .tasks
            .iter()
            .map(|item| item.title.clone())
            .collect()
    };
    assert_eq!(
        found.each_ref().map(names),
        [
            titles(&["mine", "nobodys"]),
            titles(&["mine", "nobodys"]),
            Vec::new(),
            titles(&["mine-blocked", "mine-deferred", "mine-later-today"]),
        ]
    );
    assert_eq!((found[0].total, found[3].total), (2, 3));
    let [available, _, _, held] = sections(&tasks, &clock, &blocked);
    assert_eq!(
        (available, held),
        (titles(&["mine", "nobodys", "theirs"]), Vec::new())
    );
}
