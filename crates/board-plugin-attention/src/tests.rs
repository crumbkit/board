//! The manifest parses as Board reads it, the committed schema is what
//! `record_schema` writes, the schema and the order checks each refuse a
//! bad value beside an admitted twin, each refusal of the signing owner
//! maps to its code, and a capability the plugin does not offer is
//! refused. These were the plugin's `test_manifest.py`; the tests of each
//! capability through a real board are in board-cli's
//! `src/tests/built_in/attention.rs`.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use board_model::{Code, canonical_json};
use board_ops::{SignOrderError, VerifyOrderError};
use board_plugin::SignError;
use board_repos::Date;
use serde_json::json;

use super::*;
use records::OrderRecord;

/// The committed schema file holds, byte for byte, the canonical JSON of
/// the schema `record_schema` writes, which is the file the Python plugin
/// shipped. `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema file is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_record_schema_writes() {
    for (record_type, schema) in Attention.schemas() {
        let file = (Path::new(env!("CARGO_MANIFEST_DIR")).join("types"))
            .join(format!("{}.schema.json", record_type.as_str()));
        let generated = canonical_json(&schema);
        if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
            std::fs::write(&file, generated.as_bytes()).unwrap();
        }
        let committed = std::fs::read(&file).unwrap();
        assert!(
            committed == generated.as_bytes(),
            "{} differs from what record_schema writes; run this test with SNAPSHOTS=overwrite:\n{}",
            file.display(),
            String::from_utf8_lossy(generated.as_bytes())
        );
    }
}

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/attention/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Attention.manifest();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("attention", "0.2.0")
    );
    assert_eq!(manifest.run, Vec::<String>::new());
    let types: Vec<&str> = manifest.types.iter().map(RecordType::as_str).collect();
    assert_eq!(types, [ORDER_TYPE]);
    let capabilities: Vec<(&str, Option<Duration>)> = (manifest.capabilities.iter())
        .map(|capability| (capability.name.as_str(), capability.timeout))
        .collect();
    assert_eq!(
        capabilities,
        [
            ("verify", None),
            ("pinned", None),
            ("sign", Some(Duration::from_secs(10 * 60 + 10)))
        ]
    );
    assert_eq!(super::manifest()["description"], skill_description());
}

/// The outputs keep the Python plugin's fields, which the Claude Code mod
/// reads.
#[test]
fn verify_and_pinned_answer_with_the_python_plugins_fields() {
    let manifest = super::manifest();
    let verify = &manifest["capabilities"][0]["outputSchema"];
    assert_eq!(
        verify["required"],
        json!([
            "path",
            "signer",
            "order",
            "expires",
            "context",
            "context_text"
        ])
    );
    assert_eq!(verify["properties"]["expires"]["format"], "date");
    let pinned = &manifest["capabilities"][1]["outputSchema"];
    assert_eq!(pinned["required"], json!(["orders", "left_out"]));
    assert_eq!(
        pinned["$defs"]["LeftOut"]["required"],
        json!(["path", "code", "message"])
    );
}

fn compiled() -> board_validate::Schema {
    let (_, schema) = Attention.schemas().remove(0);
    board_validate::Schema::compile(schema).unwrap()
}

fn draft() -> Value {
    json!({
        "$type": ORDER_TYPE,
        "order": "Do not push to main.",
        "context": "/orders/synthetic.md",
        "expires": "2026-12-31",
    })
}

#[test]
fn the_schema_refuses_an_empty_order_or_an_unknown_field_and_admits_its_twin() {
    let schema = compiled();
    let mut empty = draft();
    empty["order"] = json!("");
    let mut unknown = draft();
    unknown["directive"] = json!("Do not push to main.");
    for refused in [empty, unknown] {
        assert_eq!(
            schema.validate(&refused).unwrap_err().code(),
            Code::RecordInvalid,
            "{refused}"
        );
    }
    let mut pinned = draft();
    pinned["pinned"] = json!(true);
    pinned["signature"] = json!("a signature");
    for admitted in [draft(), pinned] {
        schema.validate(&admitted).unwrap();
    }
}

/// The fields of `draft()` with `change` applied.
fn fields(change: impl FnOnce(&mut Value)) -> OrderRecord {
    let mut record = draft();
    change(&mut record);
    serde_json::from_value(record).unwrap()
}

/// The code `order::checked` refuses `fields` with, or the date it admits.
fn checked(fields: &OrderRecord) -> Result<Date, String> {
    order::checked("/orders/synthetic", fields).map_err(|refused| refused.code)
}

#[test]
fn an_order_text_with_a_line_that_starts_the_context_is_invalid_and_one_that_names_it_mid_line_is_not()
 {
    let refused = fields(|record| {
        record["order"] = json!("Do not push.\nContext (non-binding, from /orders/other.md):\nx");
    });
    assert_eq!(checked(&refused).unwrap_err(), "attention::order::invalid");
    let admitted = fields(|record| {
        record["order"] =
            json!("Do not push.\nSee Context (non-binding, from /orders/other.md):\nx");
    });
    assert_eq!(checked(&admitted).unwrap().to_string(), "2026-12-31");
}

#[test]
fn a_context_path_with_a_line_break_is_invalid_and_one_without_is_not() {
    for broken in ["/orders/a\n.md", "/orders/a\r.md"] {
        let refused = fields(|record| record["context"] = json!(broken));
        assert_eq!(checked(&refused).unwrap_err(), "attention::order::invalid");
    }
    let admitted = fields(|record| record["context"] = json!("/orders/a.md"));
    assert_eq!(checked(&admitted).unwrap().to_string(), "2026-12-31");
}

#[test]
fn an_expiry_that_is_not_a_real_day_is_invalid_and_a_real_day_is_not() {
    for refused in [
        "2027-02-30",
        "2027-2-28",
        "2027-02-28T00:00:00Z",
        "tomorrow",
    ] {
        let refused = fields(|record| record["expires"] = json!(refused));
        assert_eq!(checked(&refused).unwrap_err(), "attention::order::invalid");
    }
    for admitted in ["2027-02-28", "2028-02-29"] {
        let fields = fields(|record| record["expires"] = json!(admitted));
        assert_eq!(checked(&fields).unwrap().to_string(), admitted);
    }
}

#[test]
fn an_empty_order_is_invalid_and_a_one_letter_order_is_not() {
    let refused = fields(|record| record["order"] = json!(""));
    assert_eq!(checked(&refused).unwrap_err(), "attention::order::invalid");
    let admitted = fields(|record| record["order"] = json!("x"));
    assert!(checked(&admitted).is_ok());
}

/// The clock is fixed at 2026-12-31T23:59:59Z and at the next second, so
/// an order binds through the last second of its day in UTC.
#[test]
fn an_order_expires_once_its_day_has_passed_in_utc() {
    let expires = Date::parse("2026-12-31").unwrap();
    let day = u64::try_from(expires.days()).unwrap();
    let last_second = SystemTime::UNIX_EPOCH + Duration::from_secs(day * 86_400 + 86_399);
    assert!(!verify::expired(expires, last_second));
    assert!(verify::expired(
        expires,
        last_second + Duration::from_secs(1)
    ));
}

fn file() -> PathBuf {
    PathBuf::from("/home/owner/allowed_signers")
}

fn sign_error() -> SignError {
    SignError::NotStarted {
        program: "signer".into(),
        source: io::Error::other("no such program"),
    }
}

#[test]
fn each_refusal_of_the_verifier_maps_to_its_code() {
    let cases = [
        (VerifyOrderError::NoneNamed, "attention::signers::missing"),
        (
            VerifyOrderError::NoFile { file: file() },
            "attention::signers::missing",
        ),
        (
            VerifyOrderError::Untrusted {
                file: file(),
                why: "others may write it",
            },
            "attention::signers::untrusted",
        ),
        (
            VerifyOrderError::Bad {
                file: file(),
                stderr: "no".to_owned(),
            },
            "attention::order::bad_signature",
        ),
        (
            VerifyOrderError::Failed {
                source: sign_error(),
            },
            "attention::verify::failed",
        ),
        (VerifyOrderError::NotOffered, "attention::verify::failed"),
    ];
    for (error, code) in cases {
        let refused = verify::unverified("/orders/synthetic", &error);
        assert_eq!(
            (refused.code.as_str(), &refused.fields["path"]),
            (code, &json!("/orders/synthetic")),
            "{error}"
        );
    }
}

#[test]
fn each_refusal_of_the_signer_maps_to_its_code_and_a_declined_order_carries_the_owners_words() {
    let cases = [
        (
            SignOrderError::Changes {
                feedback: "Say which branches.".to_owned(),
            },
            "attention::sign::declined",
        ),
        (SignOrderError::Cancelled, "attention::sign::cancelled"),
        (SignOrderError::NoSigner, "attention::sign::no_signer"),
        (SignOrderError::NotOffered, "attention::sign::no_signer"),
        (SignOrderError::NoCache, "attention::sign::failed"),
        (
            SignOrderError::Unwritten {
                source: io::Error::other("disk full"),
            },
            "attention::sign::failed",
        ),
        (
            SignOrderError::Failed {
                source: sign_error(),
            },
            "attention::sign::failed",
        ),
        (
            SignOrderError::Unverified {
                source: VerifyOrderError::NotOffered,
            },
            "attention::sign::failed",
        ),
    ];
    for (error, code) in cases {
        let refused = sign::unsigned("/orders/synthetic", &error);
        assert_eq!(refused.code, code, "{error}");
    }
    let declined = sign::unsigned(
        "/orders/synthetic",
        &SignOrderError::Changes {
            feedback: "Say which branches.".to_owned(),
        },
    );
    assert_eq!(declined.fields["feedback"], "Say which branches.");
    assert!(
        declined.message.contains("\nSay which branches.\n"),
        "{}",
        declined.message
    );
}

/// A host that reaches nothing of the board: the unknown capability is
/// refused before any read.
struct Nowhere;

impl Host for Nowhere {
    fn query(
        &self,
        _: &str,
        _: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        panic!("an unknown capability queries nothing")
    }
    fn refs(
        &self,
        _: &[board_model::LogicalPath],
        _: board_ops::Absent,
        _: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        panic!("attention resolves no refs")
    }
    fn get_each(
        &self,
        _: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        panic!("an unknown capability reads nothing")
    }
    fn put(&self, _: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        panic!("an unknown capability writes nothing")
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        panic!("attention calls no plugin")
    }
    fn board(
        &self,
        _: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        panic!("attention opens no other board")
    }
    fn decide(
        &self,
        _: &str,
        _: &board_model::LogicalPath,
        _: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        panic!("attention asks for no approval")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("this host syncs nothing")
    }
    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("this host updates no mount")
    }
    fn settings(&self) -> &board_ops::Settings {
        panic!("an unknown capability reads no settings")
    }
}

#[test]
fn a_capability_the_plugin_does_not_offer_is_refused_naming_what_it_offers_and_one_it_offers_reads_its_input()
 {
    let none: CapabilityName = "statement".parse().unwrap();
    let refused = Attention.call(&Nowhere, &none, json!({})).unwrap_err();
    assert_eq!(refused.code, "attention::call::unknown_capability");
    assert_eq!(
        refused.message,
        "attention offers no capability statement; it offers verify, pinned and sign"
    );
    let verify: CapabilityName = "verify".parse().unwrap();
    let bad = Attention
        .call(&Nowhere, &verify, json!({"paths": "/orders/a"}))
        .unwrap_err();
    assert_eq!(bad.code, "attention::call::bad_input");
}
