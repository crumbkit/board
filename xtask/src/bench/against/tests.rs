use std::time::Duration;

use super::*;
use crate::tests::Fixture;

fn millis(times: &[u64]) -> Vec<Duration> {
    times.iter().copied().map(Duration::from_millis).collect()
}

#[test]
fn rounds_alternate_which_arm_runs_first_and_each_arm_runs_once_a_round() {
    assert_eq!(order(0), [Arm::Base, Arm::New]);
    assert_eq!(order(1), [Arm::New, Arm::Base]);
    let firsts = (0..15)
        .filter(|round| order(*round)[0] == Arm::Base)
        .count();
    assert_eq!(firsts, 8);
    for round in 0..15 {
        let [first, second] = order(round);
        assert_ne!(first, second);
    }
}

#[test]
fn eleven_rounds_all_much_slower_are_the_same_and_twelve_are_slower() {
    let judged = |rounds| judge(&millis(&vec![100; rounds]), &millis(&vec![200; rounds]));
    assert_eq!(judged(11).verdict, Verdict::Same);
    assert_eq!(judged(12).verdict, Verdict::Slower);
}

#[test]
fn a_row_is_slower_when_its_median_round_is_over_5_percent_slower_in_12_of_15_rounds() {
    let base = millis(&[100; 15]);
    let slower = |by: u64, rounds: usize| {
        let mut new = millis(&[100 + by; 15]);
        for time in &mut new[rounds..] {
            *time = Duration::from_millis(90);
        }
        judge(&base, &new)
    };
    assert_eq!(slower(6, 12).verdict, Verdict::Slower);
    assert_eq!(slower(6, 11).verdict, Verdict::Same, "11 of 15 rounds");
    assert_eq!(slower(5, 15).verdict, Verdict::Same, "exactly 5 percent");
    assert_eq!(slower(6, 12).slower, 12);
}

#[test]
fn a_row_is_faster_by_the_same_rule_in_the_other_direction() {
    let base = millis(&[100; 15]);
    let mut new = millis(&[90; 15]);
    for time in &mut new[12..] {
        *time = Duration::from_millis(110);
    }
    let judged = judge(&base, &new);
    assert_eq!(judged.verdict, Verdict::Faster);
    assert_eq!(judged.slower, 3);
    new[11] = Duration::from_millis(110);
    assert_eq!(judge(&base, &new).verdict, Verdict::Same);
}

#[test]
fn a_comparison_fails_on_a_slower_row_or_a_new_failure_and_passes_when_only_base_fails() {
    let row = |verdict| Compared {
        name: "x".to_owned(),
        base: Duration::ZERO,
        new: Duration::ZERO,
        delta: 0.0,
        slower: 0,
        rounds: 15,
        verdict,
    };
    assert!(passed(&[row(Verdict::Same), row(Verdict::Faster)]));
    assert!(passed(&[row(Verdict::Failed(Arm::Base, 1))]));
    assert!(!passed(&[row(Verdict::Slower)]));
    assert!(!passed(&[row(Verdict::Failed(Arm::New, 2))]));
}

#[test]
fn each_row_prints_its_verdict_medians_and_rounds_and_a_failure_names_its_arm() {
    let compared = [
        Compared {
            name: "`board get`, warm".to_owned(),
            base: Duration::from_micros(1_250),
            new: Duration::from_micros(1_500),
            delta: 0.2,
            slower: 13,
            rounds: 15,
            verdict: Verdict::Slower,
        },
        Compared {
            name: "`board check`, warm, nothing changed".to_owned(),
            base: Duration::ZERO,
            new: Duration::ZERO,
            delta: 0.0,
            slower: 0,
            rounds: 0,
            verdict: Verdict::Failed(Arm::Base, 1),
        },
    ];
    assert_eq!(
        crate::report::render_against(&compared),
        "SLOWER  base      1.2 ms  new      1.5 ms   +20.0%  13/15 rounds slower  `board get`, warm\n\
         skip    base failed (exit 1)  `board check`, warm, nothing changed\n"
    );
}

#[test]
fn a_missing_base_binary_is_refused_and_an_existing_one_is_admitted() {
    let fixture = Fixture::new();
    fixture.write("board", "");
    let missing = fixture.path().join("nothing");
    let refused = base(&missing).unwrap_err();
    assert_eq!(refused.code(), "xtask::bench::no_against");
    assert_eq!(
        base(&fixture.path().join("board")).unwrap(),
        fixture.path().join("board")
    );
}
