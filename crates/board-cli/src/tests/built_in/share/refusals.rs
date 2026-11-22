//! `share.to`'s refusals, each beside its admitted twin: of what the set
//! holds, which are decided before anything is written, and of where it
//! goes.

use super::*;

/// A board in a repository of its own with one commit, and the commit.
fn library() -> (Scenario, ObjectId) {
    let library = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": true}"#;
    library.write(".board/board.json", Item::File(config));
    let commit = library.commit(&[(".board/board.json", config)]);
    library.track(commit);
    (library, commit)
}

/// A `board/attachment/v1` record whose bytes are at `location`.
fn attachment(location: &str) -> String {
    json!({
        "$type": "board/attachment/v1",
        "digest": "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        "size": 4,
        "media_type": "image/png",
        "location": location,
    })
    .to_string()
}

#[test]
fn a_reference_through_a_mount_the_team_lacks_is_refused_and_one_the_team_mounts_too_is_kept() {
    let boards = boards();
    let (library, commit) = library();
    let pointer = mount_of(&library.path(), commit);
    boards.personal.write("repos/library.json", &pointer);
    boards.experiment("/notes/cases.md", &json!({"mounts": ["/repos/library"]}));
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::closure::unresolved");
    assert_eq!(refused["fields"]["paths"], strings(&["/repos/library"]));
    boards.publish(&[("repos/library.json", &pointer)]);
    let output = boards.share(&["--path", "/evals/x"]);
    assert!(
        !output["copied"]
            .as_array()
            .unwrap()
            .contains(&json!("/repos/library"))
    );
}

#[test]
fn an_attachment_kept_on_this_machine_is_refused_unless_asked_and_one_in_a_remote_store_is_copied()
{
    let boards = boards();
    boards.standard();
    let at = "evals/x/evidence/shot.png.json";
    boards
        .personal
        .write(at, &attachment("/Users/me/objects/9f86"));
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::attachment::local");
    assert_eq!(
        refused["fields"]["paths"],
        strings(&["/evals/x/evidence/shot.png"])
    );
    let asked = ["--path", "/evals/x", "--local_attachments", "true"];
    let copied = boards.preview(&asked)["copied"].clone();
    assert!(
        copied
            .as_array()
            .unwrap()
            .contains(&json!("/evals/x/evidence/shot.png"))
    );
    boards.personal.write(at, &attachment("s3://objects/9f86"));
    boards.share(&["--path", "/evals/x"]);
    assert!(boards.exists(at));
}

#[test]
fn a_binary_file_is_refused_because_a_batch_writes_only_text_and_a_text_file_is_copied() {
    let boards = boards();
    boards.standard();
    let at = "evals/x/evidence/raw.bin";
    boards.personal.board.write(at, Item::Bytes(b"\xff\x00"));
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::file::unsupported");
    assert_eq!(
        refused["fields"]["paths"],
        strings(&["/evals/x/evidence/raw.bin"])
    );
    boards.personal.write(at, "ok\n");
    boards.share(&["--path", "/evals/x"]);
}

#[test]
fn a_record_its_schema_refuses_is_refused_before_any_write_and_a_dangling_reference_is_admitted() {
    let boards = boards();
    boards.experiment("/notes/cases.md", &json!({"status": "sideways"}));
    let before = boards.head().id;
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::record::invalid");
    assert_eq!(refused["fields"]["paths"], strings(&["/evals/x"]));
    assert_eq!(
        refused["message"],
        "`board put` would refuse, as this board's index does, /evals/x; nothing was written"
    );
    assert_eq!(boards.head().id, before);
    boards.experiment("/notes/missing.md", &json!({}));
    let output = boards.share(&["--path", "/evals/x"]);
    assert!(
        !output["copied"]
            .as_array()
            .unwrap()
            .contains(&json!("/notes/cases.md"))
    );
}

#[test]
fn a_link_inside_the_set_is_refused_and_one_outside_it_is_left_alone() {
    let boards = boards();
    let (library, commit) = library();
    let pointer = mount_of(&library.path(), commit);
    boards.standard();
    boards.personal.write("evals/x/library.json", &pointer);
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::link::unsupported");
    assert_eq!(refused["fields"]["paths"], strings(&["/evals/x/library"]));
    boards.personal.delete("evals/x/library.json");
    boards.personal.write("library.json", &pointer);
    assert_eq!(
        boards.share(&["--path", "/evals/x"])["copied"],
        json!(SHARED)
    );
}

#[test]
fn a_markdown_record_its_schema_refuses_is_refused_before_any_write_and_its_valid_twin_is_copied() {
    let boards = boards();
    boards.standard();
    boards
        .personal
        .write(TASK_SCHEMA, TASK_TYPE)
        .write("tasks/t.md", "---\n$type: lr/task/v1\neval: 1\n---\n");
    let refused = boards.refused(&["--path", "/tasks/t.md"]);
    assert_eq!(refused["code"], "share::record::invalid");
    boards.personal.write(
        "tasks/t.md",
        "---\n$type: lr/task/v1\neval: /evals/x\n---\n",
    );
    boards.share(&["--path", "/tasks/t.md"]);
}

#[test]
fn typed_frontmatter_that_does_not_parse_is_refused_in_the_preview_and_its_parsing_twin_is_listed()
{
    let boards = boards();
    boards.standard();
    boards
        .personal
        .write(TASK_SCHEMA, TASK_TYPE)
        .write("tasks/t.md", "---\n$type: lr/task/v1\ntitle: [\n---\n");
    let dry = ["--path", "/tasks/t.md"];
    assert_eq!(
        boards.refused_preview(&dry)["code"],
        "share::record::invalid"
    );
    boards
        .personal
        .write("tasks/t.md", "---\n$type: lr/task/v1\ntitle: '['\n---\n");
    assert_eq!(boards.preview(&dry)["copied"], json!(["/tasks/t.md"]));
}

#[test]
fn a_json_file_without_a_type_is_refused_and_the_same_text_in_a_text_file_is_copied() {
    let boards = boards();
    boards.standard();
    boards
        .personal
        .write("evals/x/evidence/data.json", r#"{"a": 1}"#);
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::file::unsupported");
    assert_eq!(
        refused["fields"]["paths"],
        strings(&["/evals/x/evidence/data"])
    );
    boards.personal.delete("evals/x/evidence/data.json");
    boards
        .personal
        .write("evals/x/evidence/data.txt", r#"{"a": 1}"#);
    boards.share(&["--path", "/evals/x"]);
}

#[test]
fn a_mount_that_reaches_no_board_is_refused_and_one_that_reaches_the_team_is_admitted() {
    let boards = boards();
    boards.standard();
    for to in ["/nowhere", "/evals"] {
        let refused = boards.refused_preview(&["--path", "/evals/x", "--to", to]);
        assert_eq!(refused["code"], "share::mount::not_found", "{to}");
        assert_eq!(
            refused["message"],
            format!(
                "{to} is not the mount record of another repository; `board ls {to}` shows what is there"
            )
        );
    }
    boards.preview(&["--path", "/evals/x"]);
}

#[test]
fn a_path_inside_the_mount_is_refused_and_the_mount_itself_is_admitted() {
    let boards = boards();
    boards.standard();
    boards.publish(&[("sub/a.md", "# A\n")]);
    let inside = ["--path", "/evals/x", "--to", "/team/sub"];
    assert_eq!(
        boards.refused_preview(&inside)["code"],
        "share::mount::not_found"
    );
    boards.preview(&["--path", "/evals/x"]);
}

#[test]
fn a_mount_inside_the_mounted_board_is_refused_and_its_repository_mounted_here_is_admitted() {
    let boards = boards();
    let (inner, commit) = library();
    let pointer = mount_of(&inner.path(), commit);
    boards.publish(&[("inner.json", &pointer)]);
    boards.standard();
    let nested = ["--path", "/evals/x", "--to", "/team/inner"];
    assert_eq!(
        boards.refused_preview(&nested)["code"],
        "share::mount::not_found"
    );
    boards.personal.write("inner.json", &pointer);
    boards.preview(&["--path", "/evals/x", "--to", "/inner"]);
}

#[test]
fn a_link_to_the_mount_is_refused_and_the_mount_it_links_to_is_admitted() {
    let boards = boards();
    boards.standard();
    boards.personal.write(
        "alias.json",
        r#"{"$type": "board/mount/v1", "path": "/team"}"#,
    );
    let linked = ["--path", "/evals/x", "--to", "/alias"];
    assert_eq!(
        boards.refused_preview(&linked)["code"],
        "share::mount::not_found"
    );
    boards.preview(&["--path", "/evals/x"]);
}

#[test]
fn a_team_checkout_on_another_branch_refuses_the_share_and_one_on_the_followed_branch_takes_it() {
    let boards = boards();
    boards.standard();
    let before = boards.head().id;
    (boards.team)
        .write(
            ".git/refs/heads/feature",
            Item::File(&format!("{before}\n")),
        )
        .write(".git/HEAD", Item::File("ref: refs/heads/feature\n"));
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::board::refused");
    let message = refused["message"].as_str().unwrap();
    let said = format!(
        "`board put --batch` refused with {}: ",
        refused["fields"]["code"].as_str().unwrap()
    );
    assert!(message.starts_with(&said), "{message}");
    assert_eq!(
        (boards.head().id.to_string(), boards.pin()),
        (before.to_string(), before.to_string())
    );
    boards
        .team
        .write(".git/HEAD", Item::File("ref: refs/heads/main\n"));
    boards.share(&["--path", "/evals/x"]);
    assert_eq!(boards.pin(), boards.head().id.to_string());
    assert_ne!(boards.head().id, before);
}

#[test]
fn a_team_board_that_does_not_commit_on_put_refuses_the_share_and_one_that_does_takes_it() {
    let boards = boards();
    boards.standard();
    let off = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    let before = boards.publish(&[(".board/board.json", off)]);
    let refused = boards.refused(&["--path", "/evals/x"]);
    assert_eq!(refused["code"], "share::board::refused");
    assert!(
        !boards.exists("evals/x.json"),
        "nothing is written in the team board"
    );
    let on = r#"{"$type": "board/config/v1", "commit_on_put": true}"#;
    boards.publish(&[(".board/board.json", on)]);
    boards.share(&["--path", "/evals/x"]);
    assert_ne!(boards.head().id, before);
}

#[test]
fn a_mount_whose_repository_is_missing_is_refused_with_boards_code_and_the_team_is_admitted() {
    let boards = boards();
    boards.standard();
    let commit = boards.head().id;
    let gone = boards.team.workdir().join("gone");
    boards
        .personal
        .write("gone.json", &mount_of(&gone.display().to_string(), commit));
    let refused = boards.refused_preview(&["--path", "/evals/x", "--to", "/gone"]);
    assert_eq!(
        (&refused["code"], &refused["fields"]["code"]),
        (
            &json!("share::board::refused"),
            &json!("board::mount::repo_missing")
        ),
        "{refused}"
    );
    boards.preview(&["--path", "/evals/x"]);
}

#[test]
fn links_share_cannot_copy_refuse_the_share_naming_each_and_allow_uncopied_writes_the_set() {
    let boards = boards();
    boards.uncopied();
    let before = boards.head().id;
    let refused = boards.refused(&["--path", "/tasks/p.md"]);
    assert_eq!(
        (&refused["code"], &refused["fields"]["paths"]),
        (
            &json!("share::closure::uncopied"),
            &strings(&["/projects", "/team/notes/team.md"])
        )
    );
    let message = refused["message"].as_str().unwrap();
    for named in ["/projects (directory)", "/team/notes/team.md (mount)"] {
        assert!(message.contains(named), "{message}");
    }
    assert_eq!(boards.head().id, before);
    assert!(!boards.exists("tasks/p.md"));
    let output = boards.share(&["--path", "/tasks/p.md", "--allow-uncopied"]);
    assert_eq!(
        (&output["copied"], &output["not_copied"], &output["next"]),
        (&json!(PROJECT), &not_copied(), &Value::Null)
    );
    assert!(boards.exists("tasks/p.md"));
}

#[test]
fn a_back_refs_reference_through_a_mount_the_team_lacks_is_listed_as_not_copied_and_allow_uncopied_writes()
 {
    let boards = tasks_boards();
    let (library, commit) = library();
    boards
        .personal
        .write("repos/library.json", &mount_of(&library.path(), commit));
    boards.reading_list().personal.write(
        "projects/tidy/decisions/d.json",
        &plan_record(&json!({"refs": ["/tasks/tidy.md", "/repos/library"]})),
    );
    let call = ["--path", "/tasks/remove.md"];
    let preview = boards.preview(&call);
    assert_eq!(
        (&preview["not_copied"], &preview["next"]),
        (
            &json!([{"path": "/repos/library", "reason": "mount", "from": "/projects/tidy/decisions/d"}]),
            &json!(
                boards.next("share to --path /tasks/remove.md --to /team --yes --allow-uncopied")
            )
        )
    );
    let before = boards.head().id;
    let refused = boards.refused(&call);
    assert_eq!(
        (&refused["code"], &refused["fields"]["paths"]),
        (
            &json!("share::closure::uncopied"),
            &strings(&["/repos/library"])
        )
    );
    assert_eq!(boards.head().id, before);
    boards.share(&["--path", "/tasks/remove.md", "--allow-uncopied"]);
    assert!(boards.exists("projects/tidy/decisions/d.json"));
}

#[test]
fn the_shared_tasks_own_reference_through_a_mount_the_team_lacks_is_still_refused() {
    let boards = tasks_boards();
    let (library, commit) = library();
    boards
        .personal
        .write("repos/library.json", &mount_of(&library.path(), commit));
    boards.reading_list().personal.write(
        "tasks/remove.md",
        &task("Remove", "parent: /tasks/tidy.md\nbrief: /repos/library\n"),
    );
    let call = ["--path", "/tasks/remove.md", "--allow-uncopied"];
    let refused = boards.refused_preview(&call);
    assert_eq!(
        (&refused["code"], &refused["fields"]["paths"]),
        (
            &json!("share::closure::unresolved"),
            &strings(&["/repos/library"])
        )
    );
    assert_eq!(boards.refused(&call)["code"], "share::closure::unresolved");
}
