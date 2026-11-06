//! A source's key is known only from what its probes read.

use std::collections::{HashMap, HashSet};

use board_model::{Code, Digest, ObjectId, Source};
use board_scenario::Scenario;

use crate::git::Repository;
use crate::mounts::{Mounted, Reached};

use super::Probe;

#[test]
fn a_source_holding_a_pointer_that_could_not_be_read_has_no_key_and_its_twin_has_one() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("a.json", "{}")]);
    for (unread, keyed) in [(true, false), (false, true)] {
        let source = Mounted::<()> {
            commit,
            root: commit,
            repository: Repository::open(scenario.workdir()).unwrap(),
            route: "/vendor".parse().unwrap(),
            key: None,
            scan: Digest::of(b""),
            crossings: HashSet::new(),
            kept: false,
            whole: true,
            walked: None,
        };
        let reached = Reached {
            sources: vec![source],
            points: Vec::new(),
            edges: Vec::new(),
            probes: Vec::new(),
            unread: if unread {
                vec![Source::Commit(commit)]
            } else {
                Vec::new()
            },
            scopes: HashMap::new(),
            cycles: HashMap::new(),
        };
        let closure = reached.closure(commit, &mut HashMap::new());
        assert_eq!(closure.is_some(), keyed, "unread: {unread}");
    }
}

#[test]
fn a_source_whose_mount_is_a_cycle_has_a_key_of_its_own_and_one_for_each_board_it_repeats() {
    let scenario = Scenario::new();
    let commit = scenario.commit(&[("a.json", "{}")]);
    let closure = |landed: Result<(), Code>, repeats: Option<&str>| {
        let source = Mounted::<()> {
            commit,
            root: commit,
            repository: Repository::open(scenario.workdir()).unwrap(),
            route: "/vendor".parse().unwrap(),
            key: None,
            scan: Digest::of(b""),
            crossings: HashSet::new(),
            kept: false,
            whole: true,
            walked: None,
        };
        let from = Source::Commit(commit);
        let probe = Probe {
            from: from.clone(),
            path: "peer.json".to_owned(),
            repo: "../peer".to_owned(),
            commit: ObjectId::Sha1([7; 20]),
            landed: landed.map(|()| scenario.workdir().to_path_buf()),
        };
        let cycles = (repeats.into_iter())
            .map(|repeats| {
                (
                    (from.clone(), "peer.json".to_owned()),
                    repeats.parse().unwrap(),
                )
            })
            .collect();
        let reached = Reached {
            sources: vec![source],
            points: Vec::new(),
            edges: Vec::new(),
            probes: vec![probe],
            unread: Vec::new(),
            scopes: HashMap::new(),
            cycles,
        };
        reached.closure(commit, &mut HashMap::new()).unwrap()
    };
    let missing = closure(Err(Code::MountCommitMissing), None);
    let root = closure(Err(Code::MountCycle), Some("/"));
    let vendor = closure(Err(Code::MountCycle), Some("/vendor"));
    assert_ne!(root, missing);
    assert_ne!(root, vendor);
    assert_eq!(root, closure(Err(Code::MountCycle), Some("/")));
}
