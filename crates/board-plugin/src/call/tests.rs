use super::*;

#[test]
fn a_target_is_a_plugin_and_capability_joined_by_a_dot() {
    assert_eq!(
        "tasks.due".parse::<Target>().unwrap(),
        Target::Named {
            plugin: "tasks".parse().unwrap(),
            capability: "due".parse().unwrap(),
        }
    );
    for refused in ["tasks", "tasks.", ".due", "tasks.due.x", "Tasks.due"] {
        assert!(refused.parse::<Target>().is_err(), "{refused}");
    }
}

#[test]
fn a_depth_below_the_limit_is_admitted_and_one_at_it_refused() {
    let at = |depth: &str| Environment {
        depth: Some(OsString::from(depth)),
        ..Environment::default()
    };
    assert_eq!(Environment::default().depth().unwrap(), 0);
    assert_eq!(at("7").depth().unwrap(), 7);
    assert!(matches!(
        at("8").depth(),
        Err(CallError::TooDeep { depth: 8 })
    ));
    assert!(matches!(at("8 ").depth(), Err(CallError::BadDepth { .. })));
}
