//! A stop holds a change in its phase and in every later one, and is
//! reached there; a `through` stop only once its phase's gate passes.

use super::*;
use crate::flow::development;

fn through(phase: &str) -> Stop {
    Stop::Through(phase.to_owned())
}

fn until(phase: &str) -> Stop {
    Stop::Until(phase.to_owned())
}

#[test]
fn a_stop_holds_a_change_in_its_phase_and_later_and_lets_an_earlier_one_advance() {
    let flow = development();
    for stop in [through("build"), until("build")] {
        assert!(!holds(&flow, "plan", &stop), "{stop}");
        assert!(holds(&flow, "build", &stop), "{stop}");
        assert!(holds(&flow, "review", &stop), "{stop}");
        assert!(!holds(&flow, "paused", &stop), "{stop}");
    }
    assert!(holds(&flow, "land", &until("land")));
    assert!(!holds(&flow, "land", &until("done")));
}

#[test]
fn a_through_stop_is_reached_once_its_gate_passes_and_an_until_stop_on_entry() {
    let flow = development();
    assert!(!reached(&flow, "plan", &through("plan"), false));
    assert!(reached(&flow, "plan", &through("plan"), true));
    assert!(reached(&flow, "build", &through("plan"), false));
    assert!(!reached(&flow, "plan", &until("build"), true));
    assert!(reached(&flow, "build", &until("build"), false));
    assert!(!reached(&flow, "land", &until("done"), true));
}

#[test]
fn a_stop_shows_as_its_word_and_phase_and_reads_as_a_rule() {
    assert_eq!(through("plan").to_string(), "through plan");
    assert_eq!(until("review").to_string(), "until review");
    assert!(through("plan").rule().contains("do not advance out of it"));
    assert!(until("review").rule().contains("do none of its work"));
    assert!(until("done").rule().contains("to done"));
    let shown = Shown {
        stop: through("plan"),
        reached: true,
    };
    assert_eq!(
        serde_json::to_value(shown).unwrap(),
        json!({"through": "plan", "reached": true})
    );
}
