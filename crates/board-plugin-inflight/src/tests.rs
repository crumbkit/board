//! The manifest parses as Board reads it, its input says what each flag
//! means, and a capability the plugin does not offer is refused.

use super::*;

#[test]
fn the_manifest_parses_as_a_built_in_one_with_its_one_capability_and_timeout() {
    let manifest = Inflight.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("inflight", "0.1.0")
    );
    assert!(manifest.run.is_empty());
    assert!(manifest.types.is_empty());
    let [now] = manifest.capabilities.as_slice() else {
        panic!("inflight offers one capability")
    };
    assert_eq!(now.name.as_str(), "now");
    assert_eq!(
        now.timeout,
        Some(std::time::Duration::from_secs(NOW_TIMEOUT))
    );
}

#[test]
fn the_limit_is_described_and_bounded_from_1_to_50_with_a_default_of_20() {
    let input = &manifest()["capabilities"][0]["inputSchema"];
    assert_eq!(input["additionalProperties"], false);
    let limit = &input["properties"]["limit"];
    let description = limit["description"].as_str().unwrap();
    assert!(description.ends_with('.'), "{description}");
    assert_eq!(
        [&limit["minimum"], &limit["maximum"], &limit["default"]],
        [&json!(1), &json!(50), &json!(20)]
    );
}

/// A host the plugin must not use.
struct Unused;

impl Host for Unused {
    fn query(
        &self,
        _sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("an unknown capability reads nothing")
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("an unknown capability resolves nothing")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("an unknown capability reads nothing")
    }

    fn put(&self, _lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("inflight writes nothing")
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("an unknown capability calls nothing")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("inflight opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("inflight asks the owner nothing")
    }

    fn settings(&self) -> &board_ops::Settings {
        panic!("an unknown capability reads no settings")
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_naming_the_one_it_offers() {
    let refused = (Inflight.call(&Unused, &"later".parse().unwrap(), json!({}))).unwrap_err();
    assert_eq!(refused.code, "inflight::call::unknown_capability");
    assert!(
        refused.message.contains("it offers now"),
        "{}",
        refused.message
    );
}
