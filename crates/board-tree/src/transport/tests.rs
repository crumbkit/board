use super::{Upstream, fetch_arguments, push_arguments};

fn upstream() -> Upstream {
    Upstream {
        remote: "origin".to_owned(),
        branch: "main".to_owned(),
        tracking: "refs/remotes/origin/main".to_owned(),
    }
}

#[test]
fn a_push_is_atomic_and_never_forced() {
    let arguments = push_arguments(&upstream(), "0123abc");
    assert_eq!(
        arguments,
        [
            "push",
            "--atomic",
            "--porcelain",
            "--quiet",
            "--",
            "origin",
            "0123abc:refs/heads/main"
        ]
    );
    for argument in &arguments {
        assert!(!argument.starts_with('+'), "{argument}");
        assert!(!["--force", "-f", "--force-with-lease", "--mirror"].contains(&argument.as_str()));
    }
}

#[test]
fn a_fetch_forces_only_the_remote_tracking_ref() {
    let arguments = fetch_arguments(&upstream());
    assert_eq!(
        arguments,
        [
            "-c",
            "fetch.fsckObjects=true",
            "fetch",
            "--quiet",
            "--",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main"
        ]
    );
    let forced: Vec<&String> = arguments
        .iter()
        .filter(|word| word.starts_with('+'))
        .collect();
    assert_eq!(forced.len(), 1);
    let (_, destination) = forced[0].split_once(':').unwrap();
    assert!(destination.starts_with("refs/remotes/"), "{destination}");
}
