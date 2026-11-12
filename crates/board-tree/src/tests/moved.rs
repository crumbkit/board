//! Moved prefixes: a path that names nothing resolves under the other prefix
//! (docs/plugins/calling/commands.md, Renamed plugins).

use board_model::Moved;

use super::*;

/// The scenario's repositories, following `/controls` moved to `/rules`.
fn moving(scenario: &Scenario) -> Repositories {
    let mut repositories = scenario.repositories();
    repositories.moving(vec![Moved::new(path("/controls"), path("/rules")).unwrap()]);
    repositories
}

fn physical(repositories: &Repositories, at: impl At, text: &str) -> Result<String, Code> {
    (repositories.resolve(&at.source(), &path(text)))
        .map(|location| location.path)
        .map_err(|error| error.code())
}

#[test]
fn a_path_under_a_moved_prefix_resolves_under_the_other_only_when_it_names_nothing() {
    let scenario = Scenario::new();
    let files = [("rules/x.json", "1"), ("controls/y.json", "2")];
    let commit = scenario.commit(&files);
    for (file, text) in files {
        scenario.write(file, Item::File(text));
    }
    let repositories = moving(&scenario);
    for at in [Source::Commit(commit), Source::WorkingTree] {
        let resolved = |text: &str| {
            (repositories.resolve(&at, &path(text)))
                .map(|location| location.path)
                .map_err(|error| error.code())
        };
        assert_eq!(resolved("/controls/x"), Ok("rules/x.json".to_owned()));
        assert_eq!(resolved("/rules/y"), Ok("controls/y.json".to_owned()));
        assert_eq!(resolved("/controls/y"), Ok("controls/y.json".to_owned()));
        assert_eq!(resolved("/controls/z"), Err(Code::PathNotFound));
    }
    let plain = scenario.repositories();
    assert_eq!(
        physical(&plain, commit, "/controls/x"),
        Err(Code::PathNotFound)
    );
}

#[test]
fn a_miss_under_both_prefixes_names_the_path_as_written() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("rules/x.json", "1")]);
    let error =
        (moving(&scenario).resolve(&Source::Commit(commit), &path("/controls/z"))).unwrap_err();
    assert!(
        matches!(
            &error,
            crate::ResolveError::NotFound { path: at, ancestor }
                if *at == path("/controls/z") && *ancestor == LogicalPath::root()
        ),
        "{error:?}"
    );
}

#[test]
fn a_path_under_a_moved_prefix_in_a_mounted_board_resolves_under_the_other() {
    let inner = Scenario::new();
    let pinned = inner.commit(&[("rules/x.json", "1")]);
    let outer = Scenario::new();
    let to_inner = format!(
        r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{pinned}"}}"#,
        inner.path()
    );
    let commit = outer.commit(&[("m.json", &to_inner)]);
    let repositories = moving(&outer);
    assert_eq!(
        physical(&repositories, commit, "/m/controls/x"),
        Ok("rules/x.json".to_owned())
    );
    assert_eq!(
        physical(&repositories, commit, "/m/controls/y"),
        Err(Code::PathNotFound)
    );
}

#[test]
fn a_write_lands_where_its_path_says_and_not_under_the_other_prefix() {
    let scenario = Scenario::new();
    scenario.write("rules/x.json", Item::File("1"));
    let repositories = moving(&scenario);
    let target = (repositories.snapshot().unwrap())
        .target(&path("/controls/x"), false)
        .unwrap();
    assert_eq!(target.path, "controls/x.json");
    let target = (repositories.snapshot().unwrap())
        .target(&path("/rules/x"), false)
        .unwrap();
    assert_eq!(target.path, "rules/x.json");
}
