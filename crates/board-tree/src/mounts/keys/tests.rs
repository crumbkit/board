//! A source's key is known only from what its probes read.

use std::collections::{HashMap, HashSet};

use board_model::{Digest, Source};
use board_scenario::Scenario;

use crate::git::Repository;
use crate::mounts::{Mounted, Reached};

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
        };
        let closure = reached.closure(commit, &mut HashMap::new());
        assert_eq!(closure.is_some(), keyed, "unread: {unread}");
    }
}
