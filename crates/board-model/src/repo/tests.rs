use super::*;
use crate::code::Code;
use crate::pointer::{Pointer, pointer, repinned};
use crate::record::{Content, Kind, Role};

/// The origin URLs and their stored forms that `board_plugin.repos`'s
/// tests read too.
const REMOTES: &str = include_str!("../../../../python/board-plugin/tests/remotes.json");

#[test]
fn a_remote_takes_the_same_stored_form_as_the_python_function_for_every_row_of_the_shared_table() {
    let rows: Vec<serde_json::Map<String, Value>> = serde_json::from_str(REMOTES).unwrap();
    assert!(rows.len() > 20);
    for row in rows {
        let (remote, stored) = (row["remote"].as_str().unwrap(), row["stored"].as_str());
        assert_eq!(Some(shown_remote(remote).as_str()), stored, "{remote}");
    }
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
fn a_checkout_of_a_remote_repository_is_refused_as_a_mount_of_one_is() {
    assert!(pointer(repo(&checkout("/srv/board")).as_bytes()).is_ok());
    let error = pointer(repo(&checkout("https://example.com/board.git")).as_bytes()).unwrap_err();
    assert_eq!(error.code(), Code::MountRemoteUnsupported);
}

#[test]
fn a_checkout_commit_must_be_a_full_id_as_a_mount_commit_must() {
    let commit = |commit: &str| {
        repo(&format!(
            r#", "checkout": {{"repo": "/r", "follow": "main", "commit": "{commit}"}}"#
        ))
    };
    assert!(pointer(commit(&"ab".repeat(20)).as_bytes()).is_ok());
    let error = pointer(commit("abc").as_bytes()).unwrap_err();
    assert_eq!(error.code(), Code::MountInvalid);
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
    fn any_text_takes_a_stored_form_no_longer_than_itself(
        remote in "(\\PC|[@:/.-]|github\\.com){0,24}"
    ) {
        proptest::prop_assert!(shown_remote(&remote).len() <= remote.len(), "{}", remote);
    }
}
