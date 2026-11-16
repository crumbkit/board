//! Each rule places a question, a question no rule places lands on its
//! realm's line, and each refusal has an admitted twin.

use crate::records::TaskRule;

use super::*;

fn rules() -> QuestionRules {
    QuestionRules {
        mount_realms: BTreeMap::from([
            ("team".to_owned(), "beta".to_owned()),
            ("other".to_owned(), "alpha".to_owned()),
        ]),
        local_realm: BTreeMap::from([("notes".to_owned(), "alpha".to_owned())]),
        local_default: "beta".to_owned(),
        tasks: vec![TaskRule {
            contains: "profile-task".to_owned(),
            to: "speed".to_owned(),
        }],
        names: BTreeMap::from([
            ("guide".to_owned(), "docs".to_owned()),
            ("team/guide".to_owned(), "docs".to_owned()),
        ]),
    }
}

fn tree(rules: &QuestionRules) -> Tree<'_> {
    Tree {
        owner: BTreeMap::from([
            ("cache".to_owned(), "speed".to_owned()),
            ("bench".to_owned(), "speed".to_owned()),
            ("guide-draft".to_owned(), "docs".to_owned()),
        ]),
        realm_of: BTreeMap::from([
            ("speed".to_owned(), "alpha".to_owned()),
            ("docs".to_owned(), "beta".to_owned()),
        ]),
        rules,
    }
}

fn question(path: &str, text: &str, work: &str) -> Waiting {
    Waiting {
        path: path.to_owned(),
        asked: "2026-02-01".to_owned(),
        text: text.to_owned(),
        blocked_work: work.to_owned(),
    }
}

fn one(waiting: Waiting) -> Placed {
    let rules = rules();
    place(&tree(&rules), &[waiting]).unwrap().remove(0)
}

#[test]
fn a_question_that_names_changes_of_one_project_is_placed_by_the_change_rule() {
    let placed = one(question(
        "/projects/x/questions/q",
        "Should cache wait for bench?",
        "",
    ));
    assert_eq!(
        (placed.project.as_deref(), placed.by, placed.realm.as_str()),
        (Some("speed"), Some(By::Change), "alpha")
    );
}

#[test]
fn a_question_that_names_changes_of_two_projects_falls_to_the_next_rule_and_one_with_a_word_boundary_miss_names_none()
 {
    let both = one(question(
        "/projects/x/questions/q",
        "cache or guide-draft?",
        "",
    ));
    assert_eq!((both.project, both.by), (None, None), "two projects");
    let inside = one(question(
        "/projects/x/questions/q",
        "Is the cached copy stale?",
        "",
    ));
    assert_eq!(
        inside.project, None,
        "`cache` inside `cached` is not a name"
    );
    let hyphen = one(question(
        "/projects/x/questions/q",
        "Is cache-key right?",
        "",
    ));
    assert_eq!(
        hyphen.project, None,
        "`cache` before a hyphen is not a name"
    );
    let twin = one(question("/projects/x/questions/q", "Is (cache) right?", ""));
    assert_eq!(twin.project.as_deref(), Some("speed"));
}

#[test]
fn a_task_in_the_blocked_work_places_a_question_and_the_change_rule_comes_first() {
    let by_task = one(question(
        "/projects/x/questions/q",
        "Which?",
        "see /tasks/profile-task.md",
    ));
    assert_eq!(
        (by_task.project.as_deref(), by_task.by),
        (Some("speed"), Some(By::Task))
    );
    let both = one(question(
        "/projects/x/questions/q",
        "guide-draft?",
        "profile-task",
    ));
    assert_eq!(
        (both.project.as_deref(), both.by),
        (Some("docs"), Some(By::Change))
    );
}

#[test]
fn the_project_folder_of_the_path_places_a_question_with_or_without_a_mount() {
    let local = one(question("/projects/guide/questions/q", "Which?", ""));
    assert_eq!(
        (local.project.as_deref(), local.by),
        (Some("docs"), Some(By::Name))
    );
    let mounted = one(question(
        "/repos/team/projects/guide/questions/q",
        "Which?",
        "",
    ));
    assert_eq!(
        (mounted.project.as_deref(), mounted.by),
        (Some("docs"), Some(By::Name))
    );
    let other = one(question(
        "/repos/other/projects/guide/questions/q",
        "Which?",
        "",
    ));
    assert_eq!(other.project, None, "the name rule keys on the mount too");
}

#[test]
fn a_question_no_rule_places_counts_on_its_realms_line_with_where_it_came_from() {
    let mounted = one(question(
        "/repos/team/projects/misc/questions/q",
        "Which?",
        "",
    ));
    assert_eq!(
        (mounted.realm.as_str(), mounted.from.as_deref()),
        ("beta", Some("team/misc"))
    );
    let local = one(question("/projects/notes/questions/q", "Which?", ""));
    assert_eq!(
        (local.realm.as_str(), local.from.as_deref()),
        ("alpha", Some("notes"))
    );
    let default = one(question("/projects/elsewhere/questions/q", "Which?", ""));
    assert_eq!(
        (default.realm.as_str(), default.from.as_deref()),
        ("beta", Some("elsewhere"))
    );
    let bare = one(question("/questions/q", "Which?", ""));
    assert_eq!(bare.from.as_deref(), Some("(no project)"));
}

#[test]
fn a_mount_without_a_realm_lands_on_the_default_realms_line_and_sums_still_hold() {
    let rules = rules();
    let strays = [
        question("/repos/stray/projects/misc/questions/q", "Which?", ""),
        question("/repos/team/projects/misc/questions/q", "Which?", ""),
    ];
    let placed = place(&tree(&rules), &strays).unwrap();
    let by_path = |path: &str| placed.iter().find(|each| each.id == path).unwrap();
    let stray = by_path("/repos/stray/projects/misc/questions/q");
    assert_eq!(
        (
            stray.realm.as_str(),
            stray.project.as_deref(),
            stray.from.as_deref()
        ),
        ("beta", None, Some("stray/misc"))
    );
    let listed = by_path("/repos/team/projects/misc/questions/q");
    assert_eq!(listed.realm, "beta");
    let other = place(
        &tree(&rules),
        &[question(
            "/repos/other/projects/misc/questions/q",
            "Which?",
            "",
        )],
    )
    .unwrap();
    assert_eq!(other[0].realm, "alpha");
}

#[test]
fn a_rule_that_names_an_unknown_project_is_refused_when_it_places_a_question() {
    let mut rules = rules();
    rules.names.insert("guide".to_owned(), "nowhere".to_owned());
    let refused = place(
        &tree(&rules),
        &[question("/projects/guide/questions/q", "Which?", "")],
    )
    .unwrap_err();
    assert_eq!(refused.code, "keeper::map::unknown_project");
}

#[test]
fn questions_are_sorted_by_day_then_path_whatever_order_they_arrive_in() {
    let rules = rules();
    let mut early = question("/projects/b/questions/q", "One", "");
    early.asked = "2026-01-01".to_owned();
    let mut late = question("/projects/a/questions/q", "Two", "");
    late.asked = "2026-03-01".to_owned();
    let mut same = question("/projects/a/questions/r", "Three", "");
    same.asked = "2026-03-01".to_owned();
    let forward = place(&tree(&rules), &[early.clone(), late.clone(), same.clone()]).unwrap();
    let backward = place(&tree(&rules), &[same, late, early]).unwrap();
    assert_eq!(forward, backward);
    let ids: Vec<&str> = forward.iter().map(|each| each.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "/projects/b/questions/q",
            "/projects/a/questions/q",
            "/projects/a/questions/r"
        ]
    );
}
