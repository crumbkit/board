use super::*;

fn failure() -> io::Error {
    io::Error::other("the disk is full")
}

#[test]
fn a_write_that_fails_is_put_failed_and_a_read_that_fails_is_unreadable() {
    let path = PathBuf::from("/board/tasks/a.json");
    let write = WriteError::Io {
        path: path.clone(),
        source: failure(),
    };
    let unsynced = WriteError::Unsynced {
        path: path.clone(),
        source: failure(),
    };
    let read = WriteError::from(GitError::File {
        path,
        source: failure(),
    });
    assert_eq!(write.code(), Code::PutFailed);
    assert_eq!(unsynced.code(), Code::PutFailed);
    assert!(
        unsynced.to_string().contains("may already be in place"),
        "nobody may read it as nothing written"
    );
    assert_eq!(read.code(), Code::RepoUnreadable);
}

#[test]
fn a_refused_write_names_its_path_and_a_failed_one_names_nothing() {
    let path: LogicalPath = "/vendor/x".parse().unwrap();
    let refused = WriteError::ThroughMount { path: path.clone() };
    assert_eq!(refused.code(), Code::PutThroughMount);
    assert_eq!(
        refused.to_string(),
        "/vendor/x cannot be written: it is in a mounted board"
    );
    assert_eq!(refused.fields(), Fields::at(&path));
    let failed = WriteError::Io {
        path: PathBuf::from("/board/vendor/x.json"),
        source: failure(),
    };
    assert_eq!(failed.fields(), Fields::default());
}

#[test]
fn a_write_refused_over_a_symlink_names_its_path_and_the_symlink() {
    let path: LogicalPath = "/x/y".parse().unwrap();
    let refused = WriteError::Symlink {
        path: path.clone(),
        symlink: "x".to_owned(),
    };
    assert_eq!(refused.code(), Code::PutSymlink);
    assert_eq!(refused.fields(), Fields::at(&path));
    assert_eq!(
        refused.to_string(),
        "/x/y cannot be written: x is a symlink"
    );
}

#[test]
fn a_write_that_reaches_a_file_that_is_not_a_record_names_that_file() {
    let path: LogicalPath = "/plain.md/a".parse().unwrap();
    let file: LogicalPath = "/plain.md".parse().unwrap();
    let refused = WriteError::NotARecord {
        path,
        file: file.clone(),
    };
    assert_eq!(refused.code(), Code::RecordNotARecord);
    assert_eq!(
        refused.to_string(),
        "/plain.md/a cannot be written: /plain.md is not a record"
    );
    assert_eq!(refused.fields(), Fields::at(&file));
}

#[test]
fn a_write_into_git_names_the_file_it_would_write() {
    let path: LogicalPath = "/sub/GIT~1/y".parse().unwrap();
    let refused = WriteError::InGit {
        path: path.clone(),
        file: "sub/GIT~1/y.json".to_owned(),
    };
    assert_eq!(refused.code(), Code::PathMalformed);
    assert_eq!(
        refused.to_string(),
        "/sub/GIT~1/y cannot be written: sub/GIT~1/y.json would be in git's own directory"
    );
    assert_eq!(refused.fields(), Fields::at(&path));
}

#[test]
fn a_write_the_walk_would_degrade_gives_a_reason_for_each_code_instead_of_the_code() {
    let path: LogicalPath = "/x".parse().unwrap();
    let cases = [
        (
            Code::PathShadowed,
            "it would share its name with another entry beside it",
        ),
        (
            Code::PathCaseCollision,
            "it would differ only in case from another entry beside it",
        ),
        (Code::PathMalformed, "it would have no valid logical path"),
        (
            Code::RepoUnreadable,
            "it would be beside an entry that may share its name and cannot be read",
        ),
    ];
    for (code, reason) in cases {
        let refused = WriteError::Degraded {
            path: path.clone(),
            code,
        };
        assert_eq!(refused.code(), code);
        assert_eq!(
            refused.to_string(),
            format!("/x cannot be written: {reason}")
        );
        assert_eq!(refused.fields(), Fields::at(&path));
    }
}

#[test]
fn a_write_that_cannot_be_resolved_names_the_resolver_s_path_unless_git_failed() {
    let path: LogicalPath = "/l/x".parse().unwrap();
    let link: LogicalPath = "/l".parse().unwrap();
    let dangling = WriteError::Unresolved {
        path: path.clone(),
        source: ResolveError::Dangling {
            path: path.clone(),
            link: link.clone(),
            target: "/y".parse().unwrap(),
        },
    };
    assert_eq!(dangling.code(), Code::RefDangling);
    assert_eq!(
        dangling.to_string(),
        "/l/x cannot be written: the link at /l points to /y, which is missing"
    );
    assert_eq!(dangling.fields(), Fields::at(&link));
    let git = WriteError::Unresolved {
        path: path.clone(),
        source: ResolveError::Git(GitError::File {
            path: PathBuf::from("/board/l.json"),
            source: failure(),
        }),
    };
    assert_eq!(git.code(), Code::RepoUnreadable);
    assert_eq!(git.fields(), Fields::at(&path));
}

#[test]
fn a_write_refused_for_its_own_path_says_it_and_names_any_other_path() {
    let at = |path: &str| path.parse::<LogicalPath>().unwrap();
    let commit = ObjectId::Sha1([0xab; 20]);
    let errors = |path: LogicalPath| {
        [
            (
                ResolveError::Submodule { path: path.clone() },
                "reaches a submodule, which is not descended".to_owned(),
            ),
            (
                ResolveError::Unreadable { path: path.clone() },
                "reaches a file that cannot be read".to_owned(),
            ),
            (
                ResolveError::InvalidPointer {
                    path: path.clone(),
                    pointer: "l.json".to_owned(),
                    source: std::sync::Arc::new(board_model::PointerError::Target),
                },
                r#"reaches the invalid pointer "l.json": pointer must hold either path, or repo, follow and commit"#.to_owned(),
            ),
            (
                ResolveError::RepoMissing {
                    path: path.clone(),
                    repo: "../gone".to_owned(),
                },
                r#"mounts the repository "../gone", which cannot be opened"#.to_owned(),
            ),
            (
                ResolveError::CommitMissing {
                    path: path.clone(),
                    commit,
                },
                format!("mounts commit {commit}, which its repository does not hold"),
            ),
            (
                ResolveError::Cycle { path: path.clone() },
                "returns to a board and path it already resolved through a pointer".to_owned(),
            ),
            (
                ResolveError::TooManyHops { path },
                "follows more than 40 pointers".to_owned(),
            ),
        ]
    };
    for (source, predicate) in errors(at("/l/x")) {
        assert_eq!(source.to_string(), format!("/l/x {predicate}"));
        let refused = WriteError::Unresolved {
            path: at("/l/x"),
            source,
        };
        assert_eq!(
            refused.to_string(),
            format!("/l/x cannot be written: it {predicate}")
        );
    }
    for (source, predicate) in errors(at("/l")) {
        let refused = WriteError::Unresolved {
            path: at("/l/x"),
            source,
        };
        assert_eq!(
            refused.to_string(),
            format!("/l/x cannot be written: /l {predicate}")
        );
    }
}

#[test]
fn a_write_refused_by_an_error_that_does_not_start_with_its_path_keeps_the_message() {
    let path: LogicalPath = "/l/x".parse().unwrap();
    let git = GitError::File {
        path: PathBuf::from("/board/l.json"),
        source: failure(),
    };
    let git_message = git.to_string();
    for (source, message) in [
        (
            ResolveError::NotFound { path: path.clone() },
            "nothing is at /l/x".to_owned(),
        ),
        (
            ResolveError::Shadowed {
                path: path.clone(),
                segment: "x".to_owned(),
            },
            r#""x" in /l/x names more than one entry"#.to_owned(),
        ),
        (ResolveError::Git(git), git_message),
    ] {
        assert_eq!(source.to_string(), message);
        let refused = WriteError::Unresolved {
            path: path.clone(),
            source,
        };
        assert_eq!(
            refused.to_string(),
            format!("/l/x cannot be written: {message}")
        );
    }
}
