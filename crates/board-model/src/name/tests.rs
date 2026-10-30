use super::*;

#[test]
fn names_are_lowercase_ascii_letters_digits_and_hyphens() {
    for good in ["tasks", "reading-list", "v2", "-"] {
        assert!(good.parse::<PluginName>().is_ok(), "{good}");
        assert!(good.parse::<CapabilityName>().is_ok(), "{good}");
    }
    for bad in ["", "Tasks", "reading_list", "tasks.due", "ä", "a b"] {
        assert!(bad.parse::<PluginName>().is_err(), "{bad}");
        assert!(bad.parse::<CapabilityName>().is_err(), "{bad}");
    }
}

#[test]
fn a_plugin_name_reads_from_json_only_when_it_is_a_name() {
    let tasks: PluginName = serde_json::from_str("\"tasks\"").unwrap();
    assert_eq!(tasks.as_str(), "tasks");
    assert_eq!(serde_json::to_string(&tasks).unwrap(), "\"tasks\"");
    assert!(serde_json::from_str::<PluginName>("\"Tasks\"").is_err());
}

#[test]
fn a_plugin_code_is_its_own_plugins_name_then_a_snake_case_area_and_reason() {
    let tasks: PluginName = "tasks".parse().unwrap();
    let code = PluginCode::parse(&tasks, "tasks::due::past_2".to_owned()).unwrap();
    assert_eq!(code.as_str(), "tasks::due::past_2");
    assert_eq!(code.to_string(), "tasks::due::past_2");
    for refused in [
        "notes::due::past",
        "tasks::due",
        "tasks::due::past::now",
        "tasks::Due::past",
        "tasks::due::past-2",
        "tasks::::past",
        "tasks::due::",
    ] {
        let error = PluginCode::parse(&tasks, refused.to_owned()).unwrap_err();
        assert_eq!(error.code, refused, "{refused}");
    }
}
