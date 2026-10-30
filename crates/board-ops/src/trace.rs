//! Where one run spends its time and how much work it does, for the timing
//! log (docs/architecture/performance/timing.md). Nothing here changes an answer.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use board_index::{Index, IndexError};
use board_plugin::Spawned;

use crate::board::Board;

/// The phases and work counts of one run, each name listed once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trace {
    pub phases: Vec<Phase>,
    /// Work counts, such as `parsed` or `rows`.
    pub work: BTreeMap<&'static str, usize>,
}

/// A named phase: when it first started, how often, and its time in all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phase {
    pub name: &'static str,
    pub started: Instant,
    pub took: Duration,
    pub entered: usize,
}

impl Trace {
    /// Adds the time since `started` to the phase `name`.
    pub fn add(&mut self, name: &'static str, started: Instant) {
        self.add_took(name, started, started.elapsed());
    }

    /// Adds `took`, from `started`, to the phase `name`.
    fn add_took(&mut self, name: &'static str, started: Instant, took: Duration) {
        if let Some(phase) = self.phases.iter_mut().find(|phase| phase.name == name) {
            phase.took += took;
            phase.entered += 1;
        } else {
            self.phases.push(Phase {
                name,
                started,
                took,
                entered: 1,
            });
        }
    }

    /// Adds `count` to the work count `name`.
    pub fn count(&mut self, name: &'static str, count: usize) {
        *self.work.entry(name).or_default() += count;
    }

    /// Counts the plugin processes in `spawned` as `started`, and adds the
    /// time each took to its first byte of reply to the phase `plugin.reply`.
    pub(crate) fn started(&mut self, spawned: Vec<Spawned>) {
        self.count("started", spawned.len());
        for child in spawned {
            if let Some(took) = child.replied {
                self.add_took("plugin.reply", child.at, took);
            }
        }
    }

    /// Appends the phases and counts of `other`, which names none of this
    /// trace's.
    pub fn extend(&mut self, other: Self) {
        self.phases.extend(other.phases);
        self.work.extend(other.work);
    }
}

/// A phase that ends when this is dropped.
pub(crate) struct Timed<'a> {
    board: &'a Board,
    name: &'static str,
    started: Instant,
}

impl Drop for Timed<'_> {
    fn drop(&mut self) {
        self.board.trace.borrow_mut().add(self.name, self.started);
    }
}

impl Board {
    /// Starts the phase `name`, which lasts until the result is dropped.
    pub(crate) fn timed(&self, name: &'static str) -> Timed<'_> {
        Timed {
            board: self,
            name,
            started: Instant::now(),
        }
    }

    /// Opens the index kept under `cache`, timed as `index.open`.
    pub(crate) fn index(&self, cache: Option<&Path>) -> Result<Index, IndexError> {
        let _timed = self.timed("index.open");
        Index::open(cache, self.repository().path())
    }

    /// Takes what this board's operations have traced so far.
    pub fn take_trace(&self) -> Trace {
        self.trace.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phase_entered_twice_keeps_one_entry_with_its_first_start_and_both_times() {
        let mut trace = Trace::default();
        let first = Instant::now();
        trace.add("resolve", first);
        trace.add("parse", Instant::now());
        trace.add("resolve", Instant::now());
        let names: Vec<&str> = trace.phases.iter().map(|phase| phase.name).collect();
        assert_eq!(names, ["resolve", "parse"]);
        assert_eq!(trace.phases[0].entered, 2);
        assert_eq!(trace.phases[0].started, first);
    }

    #[test]
    fn a_check_traces_its_phases_and_work_and_taking_the_trace_clears_it() {
        let scenario = crate::tests::board();
        let board = crate::tests::open(&scenario);
        let report = board.check(None, None).unwrap();
        let trace = board.take_trace();
        let mut names: Vec<&str> = trace.phases.iter().map(|phase| phase.name).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "index.open",
                "index.refresh",
                "index.refresh.facts",
                "index.refresh.head",
                "index.refresh.load",
                "index.refresh.sync",
                "index.refresh.walk",
                "snapshot",
                "snapshot.commit",
                "snapshot.decide",
                "snapshot.mounts",
            ]
        );
        let work = report.work;
        assert_eq!(
            trace.work,
            BTreeMap::from([
                ("entries", report.entries.len()),
                ("hashed", work.hashed),
                ("parsed", work.parsed),
                ("validated", work.validated),
                ("trees", work.trees),
                ("repositories", work.repositories),
                ("pointers_read", work.pointers_read),
            ])
        );
        assert!(report.entries.len() > 1 && work.parsed > 0);
        assert_eq!(board.take_trace(), Trace::default());
    }

    /// The names of the phases `run` enters on a fresh board.
    fn phases_of(run: impl Fn(&crate::Board)) -> Vec<&'static str> {
        let scenario = crate::tests::board();
        let board = crate::tests::open(&scenario);
        run(&board);
        let trace = board.take_trace();
        let mut names: Vec<&str> = trace.phases.iter().map(|phase| phase.name).collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn a_query_fills_the_views_of_what_it_decided_and_a_check_does_not() {
        let query = phases_of(|board| {
            board
                .query("SELECT count(*) FROM entries", 1, None, None)
                .unwrap();
        });
        assert!(query.contains(&"view"), "{query:?}");
        let check = phases_of(|board| {
            board.check(None, None).unwrap();
        });
        assert!(!check.contains(&"view"), "{check:?}");
    }

    #[test]
    fn a_snapshot_mounts_then_decides_then_commits_within_its_own_time() {
        let scenario = crate::tests::board();
        let board = crate::tests::open(&scenario);
        board.check(None, None).unwrap();
        let trace = board.take_trace();
        let phase = |name: &str| {
            let phase = trace.phases.iter().find(|phase| phase.name == name);
            phase.map(|phase| (phase.started, phase.started + phase.took))
        };
        let (start, end) = phase("snapshot").unwrap();
        let parts = ["snapshot.mounts", "snapshot.decide", "snapshot.commit"]
            .map(|name| phase(name).unwrap());
        for pair in parts.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "{parts:?}");
        }
        assert!(
            start <= parts[0].0 && parts[2].1 <= end,
            "{parts:?} in {start:?}..{end:?}"
        );
    }

    #[test]
    fn counts_of_one_name_add_up_and_extending_appends_phases_and_counts() {
        let mut trace = Trace::default();
        trace.count("rows", 2);
        trace.count("rows", 4);
        trace.add("open", Instant::now());
        let mut other = Trace::default();
        other.count("parsed", 4);
        other.add("snapshot", Instant::now());
        trace.extend(other);
        assert_eq!(trace.work, BTreeMap::from([("rows", 6), ("parsed", 4)]));
        let names: Vec<&str> = trace.phases.iter().map(|phase| phase.name).collect();
        assert_eq!(names, ["open", "snapshot"]);
    }
}
