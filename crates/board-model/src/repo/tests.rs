use super::*;
use crate::code::Code;
use crate::location::ObjectId;
use crate::pointer::{Pointer, pointer, repinned};
use crate::record::{Content, Kind, Role};

/// The origin URLs and their stored forms.
const REMOTES: &str = include_str!("remotes.json");

#[test]
fn a_remote_takes_the_stored_form_in_every_row_of_the_shared_table() {
    let rows: Vec<serde_json::Map<String, Value>> = serde_json::from_str(REMOTES).unwrap();
    assert!(rows.len() > 20);
    for row in rows {
        let remote = row["remote"].as_str().unwrap();
        if let Some(stored) = row.get("stored") {
            assert_eq!(shown_remote(remote), stored.as_str().unwrap(), "{remote}");
        }
    }
}

#[test]
fn a_remote_gives_its_identity_or_its_refusal_in_every_row_of_the_shared_table() {
    let rows: Vec<serde_json::Map<String, Value>> = serde_json::from_str(REMOTES).unwrap();
    let mut refused = 0;
    for row in rows {
        let remote = row["remote"].as_str().unwrap();
        match (
            repo_identity(remote),
            row.get("identity"),
            row.get("refused"),
        ) {
            (Ok(identity), Some(wanted), None) => {
                assert_eq!(identity.as_str(), wanted.as_str().unwrap(), "{remote}");
                let path = format!("/repositories/{}", identity.as_str());
                assert_eq!(identity.path().as_str(), path, "{remote}");
                assert_eq!(
                    repo_identity(identity.as_str()).unwrap(),
                    identity,
                    "{remote}"
                );
            }
            (Err(error), None, Some(code)) => {
                assert_eq!(error.code().as_str(), code.as_str().unwrap(), "{remote}");
                refused += 1;
            }
            (found, identity, code) => panic!("{remote}: {found:?}, not {identity:?} {code:?}"),
        }
    }
    assert!(refused > 10);
}

#[test]
fn a_local_path_a_file_url_and_a_relative_path_name_no_repository_and_a_hosted_twin_does() {
    for local in ["/srv/git/app.git", "file:///srv/git/app.git", "../app"] {
        let error = repo_identity(local).unwrap_err();
        assert_eq!(error.code(), Code::RepoLocalRemote, "{local}");
        assert_eq!(
            error.to_string(),
            format!("{local} is a local path, which names no repository outside this machine")
        );
    }
    assert!(repo_identity("https://example.com/srv/git/app.git").is_ok());
}

/// git reads an origin with no scheme and no scp-like colon as a directory
/// relative to the clone, so `owner/name` names GitHub only as an argument.
#[test]
fn an_origin_with_no_scheme_and_no_scp_colon_is_a_local_path_and_a_hosted_origin_is_not() {
    for relative in ["upstream/app", "gitlab.com/group/app", "app"] {
        let error = origin_identity(relative).unwrap_err();
        assert_eq!(error.code(), Code::RepoLocalRemote, "{relative}");
        assert_eq!(
            error.to_string(),
            format!("{relative} is a local path, which names no repository outside this machine")
        );
    }
    assert_eq!(
        repo_identity("upstream/app").unwrap().as_str(),
        "upstream/app"
    );
    for (hosted, identity) in [
        ("git@github.com:upstream/app.git", "upstream/app"),
        ("https://github.com/upstream/app", "upstream/app"),
        ("gitlab.com:group/app.git", "gitlab.com/group/app"),
    ] {
        assert_eq!(origin_identity(hosted).unwrap().as_str(), identity);
    }
    assert_eq!(
        origin_identity("/srv/git/app.git").unwrap_err().code(),
        Code::RepoLocalRemote
    );
}

#[test]
fn a_refused_remote_never_shows_its_user_or_password() {
    for remote in [
        "https://bot:s3cret@gitlab.com/a//b",
        "file://bot:s3cret@host/srv/app.git",
        "https://bot:s3cret@github.com/a/b/c",
    ] {
        let message = repo_identity(remote).unwrap_err().to_string();
        assert!(
            !message.contains("s3cret") && !message.contains("bot"),
            "{message}"
        );
    }
}

#[test]
fn an_identity_with_a_segment_over_250_bytes_is_too_long_and_one_of_250_is_admitted() {
    let long = "a".repeat(251);
    let error = repo_identity(&format!("https://gitlab.com/group/{long}.git")).unwrap_err();
    assert_eq!(error.code(), Code::RepoTooLong);
    assert!(error.to_string().contains(&long), "{error}");
    assert!(error.to_string().contains("251 bytes, over 250"), "{error}");
    let fits = "a".repeat(250);
    let identity = repo_identity(&format!("https://gitlab.com/group/{fits}.git")).unwrap();
    assert_eq!(identity.path().segments().last(), Some(fits.as_str()));
    let deep = vec!["b".repeat(200); 21].join("/");
    let error = repo_identity(&format!("https://gitlab.com/{deep}")).unwrap_err();
    assert_eq!(error.code(), Code::RepoTooLong);
}

fn repository(fields: &str) -> Value {
    serde_json::from_str(&format!(r#"{{"$type": "board/repo/v2"{fields}}}"#)).unwrap()
}

#[test]
fn a_repository_record_holds_only_its_remote_in_identity_form_and_its_twins_are_invalid() {
    let admitted = RepositoryRecord::from_value(&repository(r#", "remote": "crumbkit/board""#));
    assert_eq!(admitted.unwrap().remote.as_str(), "crumbkit/board");
    let cases = [
        (r#", "remote": "CrumbKit/Board""#, Some("crumbkit/board")),
        (
            r#", "remote": "git@github.com:crumbkit/board.git""#,
            Some("crumbkit/board"),
        ),
        (r#", "remote": "crumbkit/board", "checkout": {}"#, None),
        ("", None),
        (r#", "remote": "/srv/app""#, None),
    ];
    for (fields, wanted) in cases {
        let error = RepositoryRecord::from_value(&repository(fields)).unwrap_err();
        assert_eq!(error.code(), Code::RepoInvalid, "{fields}");
        if let Some(wanted) = wanted {
            assert!(
                error.to_string().contains(&format!("`{wanted}`")),
                "{error}"
            );
        }
    }
}

#[test]
fn a_repository_record_is_its_type_and_its_remote_in_canonical_json() {
    let identity = repo_identity("git@github.com:CrumbKit/Board.git").unwrap();
    let record = RepositoryRecord::new(identity);
    assert_eq!(
        record.to_value(),
        serde_json::json!({"$type": "board/repo/v2", "remote": "crumbkit/board"})
    );
    assert_eq!(
        RepositoryRecord::from_value(&record.to_value()).unwrap(),
        record
    );
}

#[test]
fn a_repository_record_is_a_record_of_a_known_type_and_no_pointer() {
    let valid = br#"{"$type": "board/repo/v2", "remote": "crumbkit/board"}"#;
    let classified = Role::Json.classify(Some(valid));
    assert_eq!((classified.kind(), classified.code), (Kind::Record, None));
    assert!(
        REPOSITORY_TYPE
            .parse::<crate::RecordType>()
            .unwrap()
            .is_known()
    );
    assert_eq!(pointer(valid).unwrap(), None);
}

/// The committed schema holds, byte for byte, the canonical JSON of the
/// schema the struct generates. `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema file is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_the_repository_record_struct_generates() {
    let file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("types/board/repo/v2.schema.json");
    let generated = crate::canonical_json(&repository_schema());
    if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, generated.as_bytes()).unwrap();
    }
    let committed = std::fs::read(&file).unwrap();
    assert!(
        committed == generated.as_bytes(),
        "{} differs from what its struct generates; run this test with SNAPSHOTS=overwrite:\n{}",
        file.display(),
        String::from_utf8_lossy(generated.as_bytes())
    );
}

fn repo(fields: &str) -> String {
    format!(r#"{{"$type": "board/repo/v1", "remote": "crumbkit/board"{fields}}}"#)
}

fn checkout(repo: &str) -> String {
    format!(
        r#", "checkout": {{"repo": "{repo}", "follow": "origin/main", "commit": "{}"}}"#,
        "ab".repeat(20)
    )
}

#[test]
fn a_repo_record_with_a_checkout_is_a_mount_and_one_without_is_a_record_and_no_pointer() {
    let mounted = repo(&checkout("~/src/board"));
    let mount = Mount {
        repo: "~/src/board".to_owned(),
        follow: "origin/main".to_owned(),
        commit: ObjectId::Sha1([0xab; 20]),
    };
    assert_eq!(
        pointer(mounted.as_bytes()).unwrap(),
        Some(Pointer::Mount(mount))
    );
    let classified = Role::Json.classify(Some(mounted.as_bytes()));
    assert_eq!((classified.kind(), classified.code), (Kind::Pointer, None));
    let Content::Pointer { record_type, .. } = classified.content else {
        panic!("a repo record with a checkout is a pointer");
    };
    assert_eq!(record_type.as_str(), REPO_TYPE);
    let identity = repo("");
    assert_eq!(pointer(identity.as_bytes()).unwrap(), None);
    let classified = Role::Json.classify(Some(identity.as_bytes()));
    assert_eq!((classified.kind(), classified.code), (Kind::Record, None));
    assert!(REPO_TYPE.parse::<crate::RecordType>().unwrap().is_known());
}

#[test]
fn a_repo_record_with_an_unknown_field_or_no_remote_is_invalid_and_its_twin_is_admitted() {
    assert!(pointer(repo("").as_bytes()).is_ok());
    for refused in [
        repo(r#", "name": "board""#),
        r#"{"$type": "board/repo/v1"}"#.to_owned(),
        repo(r#", "checkout": {"repo": "/r", "follow": "main"}"#),
        repo(&format!(
            r#", "checkout": {{"repo": "/r", "follow": "main", "commit": "{}", "path": "/x"}}"#,
            "ab".repeat(20)
        )),
    ] {
        let error = pointer(refused.as_bytes()).unwrap_err();
        assert_eq!(error.code(), Code::MountInvalid, "{refused}");
        let classified = Role::Json.classify(Some(refused.as_bytes()));
        assert_eq!(classified.code, Some(Code::MountInvalid), "{refused}");
    }
}

#[test]
fn a_remote_not_in_its_stored_form_is_refused_and_its_stored_form_is_admitted() {
    let record = |remote: &str| format!(r#"{{"$type": "board/repo/v1", "remote": "{remote}"}}"#);
    assert!(pointer(record("example-org/app").as_bytes()).is_ok());
    for remote in [
        "git@github.com:example-org/app.git",
        "https://user:secret@gitlab.com/example-org/app.git",
        "",
    ] {
        let error = pointer(record(remote).as_bytes()).unwrap_err();
        assert_eq!(error.code(), Code::MountInvalid, "{remote}");
    }
}

#[test]
fn a_refused_remote_is_named_by_its_stored_form_and_never_by_its_user_or_password() {
    let record = r#"{"$type": "board/repo/v1", "remote": "https://bot:s3cret@gitlab.com/example-org/app.git"}"#;
    let message = pointer(record.as_bytes()).unwrap_err().to_string();
    assert!(
        !message.contains("s3cret") && !message.contains("bot"),
        "{message}"
    );
    assert!(
        message.contains("https://gitlab.com/example-org/app.git"),
        "{message}"
    );
}

#[test]
fn a_checkout_of_a_remote_repository_or_a_short_commit_is_refused_as_a_mount_of_one_is() {
    assert!(pointer(repo(&checkout("/srv/board")).as_bytes()).is_ok());
    let error = pointer(repo(&checkout("https://example.com/board.git")).as_bytes()).unwrap_err();
    assert_eq!(error.code(), Code::MountRemoteUnsupported);
    let short = repo(r#", "checkout": {"repo": "/r", "follow": "main", "commit": "abc"}"#);
    assert_eq!(
        pointer(short.as_bytes()).unwrap_err().code(),
        Code::MountInvalid
    );
}

#[test]
fn repinning_a_repo_record_moves_its_checkout_commit_and_repinning_a_mount_moves_its_commit() {
    let new = ObjectId::Sha1([0xcd; 20]);
    let repinned_repo = repinned(repo(&checkout("/r")).as_bytes(), new).unwrap();
    let value: Value = serde_json::from_slice(&repinned_repo).unwrap();
    assert_eq!(value["checkout"]["commit"], new.to_string());
    assert!(value.get("commit").is_none());
    let mount = format!(
        r#"{{"$type": "board/mount/v1", "repo": "/r", "follow": "main", "commit": "{}"}}"#,
        "ab".repeat(20)
    );
    let value: Value = serde_json::from_slice(&repinned(mount.as_bytes(), new).unwrap()).unwrap();
    assert_eq!(value["commit"], new.to_string());
}

proptest::proptest! {
    #[test]
    fn the_identity_of_an_identity_is_itself(
        remote in "(\\PC|[@:/.~-]|github\\.com|gitlab\\.com|https://|\\.git){0,24}"
    ) {
        if let Ok(identity) = repo_identity(&remote) {
            proptest::prop_assert_eq!(repo_identity(identity.as_str()), Ok(identity.clone()), "{}", remote);
            proptest::prop_assert!(!identity.as_str().contains("://"), "{}", remote);
        }
    }

    #[test]
    fn any_text_takes_a_stored_form_no_longer_than_itself(
        remote in "(\\PC|[@:/.-]|github\\.com){0,24}"
    ) {
        proptest::prop_assert!(shown_remote(&remote).len() <= remote.len(), "{}", remote);
    }

    #[test]
    fn the_stored_form_of_a_stored_form_is_itself(
        remote in "(\\PC|[@:/.-]|github\\.com|\\.git){0,24}"
    ) {
        let shown = shown_remote(&remote);
        proptest::prop_assert_eq!(shown_remote(&shown), shown, "{}", remote);
    }
}

#[test]
fn a_record_uri_is_the_scheme_the_stored_remote_and_the_path() {
    let path: crate::LogicalPath = "/tasks/fix-login".parse().unwrap();
    assert_eq!(
        record_uri("acme/team-board", &path),
        "board://acme/team-board/tasks/fix-login"
    );
    let root = crate::LogicalPath::root();
    assert_eq!(
        record_uri("acme/team-board", &root),
        "board://acme/team-board/"
    );
}
