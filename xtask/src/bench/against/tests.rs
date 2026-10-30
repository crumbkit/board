use std::time::Duration;

use super::*;

fn millis(times: &[u64]) -> Vec<Duration> {
    times.iter().copied().map(Duration::from_millis).collect()
}

#[test]
fn one_round_is_discarded_and_the_measured_rounds_alternate_which_arm_runs_first() {
    let schedule: Vec<_> = rounds(15).collect();
    assert_eq!(schedule.len(), 16);
    assert!(!schedule[0].0);
    let measured: Vec<[Arm; 2]> = (schedule.iter())
        .filter(|(measured, _)| *measured)
        .map(|(_, arms)| *arms)
        .collect();
    assert_eq!(
        measured[..2],
        [[Arm::Base, Arm::New], [Arm::New, Arm::Base]]
    );
    let base_first = |count| {
        (rounds(count).filter(|(measured, arms)| *measured && arms[0] == Arm::Base)).count()
    };
    assert_eq!((base_first(15), base_first(16)), (8, 8));
}

#[test]
fn eleven_rounds_all_much_slower_are_the_same_and_twelve_are_slower() {
    let judged = |rounds| {
        judge(
            "x",
            &millis(&vec![100; rounds]),
            &millis(&vec![200; rounds]),
        )
    };
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
        judge("x", &base, &new)
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
    let judged = judge("x", &base, &new);
    assert_eq!(judged.verdict, Verdict::Faster);
    assert_eq!(judged.slower, 3);
    new[11] = Duration::from_millis(110);
    assert_eq!(judge("x", &base, &new).verdict, Verdict::Same);
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
