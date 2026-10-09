//! The scenario builder cleans up after itself.

use super::*;

#[test]
fn a_dropped_scenario_removes_its_directory_with_a_locked_directory_inside_and_without() {
    for item in [Item::Locked, Item::File("{}")] {
        let scenario = Scenario::new();
        scenario.write("held/x", item);
        let root = scenario.workdir().to_path_buf();
        drop(scenario);
        assert!(!root.exists(), "{item:?}");
    }
}
