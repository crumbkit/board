//! A built-in plugin's decide is the `board decide` a process plugin runs:
//! the same pending effect in the personal board, and the owner's signed
//! answer checked against the same allowed signers.

use std::path::Path;

use board_model::Answer;
use board_scenario::signer::Signer;

use super::*;
use crate::effects::{Asker, Shown, Signing};
use crate::host::Elsewhere;
use crate::{KnownBoards, answer};

/// `outer` asking to run `op` at `/pulls/7`, showing `shown`, answering
/// with the effect that admitted it or with the decide's refusal.
fn deciding(op: &'static str) -> Fixture {
    answering(
        "outer",
        Box::new(move |host| {
            let path = "/pulls/7".parse().unwrap();
            let shown = Shown::Text(b"the body".to_vec());
            match host.decide(op, &path, Some(&shown)) {
                Ok(report) => Ok(json!({"count": 0, "effect": report.effect})),
                Err(refusal) => Err(PluginError::board_refused("outer", "decide", &refusal)),
            }
        }),
    )
}

/// The owner's personal board, with the test signer whose allowed signers
/// file `signing` names when `trusted`.
struct Owner {
    personal: Scenario,
    signer: Signer,
}

impl Owner {
    fn new() -> Self {
        let personal = Scenario::new();
        personal.write(
            ".board/board.json",
            Item::File(r#"{"$type": "board/config/v1", "commit_on_put": false}"#),
        );
        Self {
            personal,
            signer: Signer::new(),
        }
    }

    fn known(&self) -> KnownBoards {
        let file = json!({"personal": self.personal.workdir()}).to_string();
        KnownBoards::parse(Path::new("/known/config.json"), Ok(file.into_bytes())).unwrap()
    }

    /// How the owner signs, with the allowed signers file when `trusted`.
    fn signing(&self, trusted: bool) -> Signing {
        Signing {
            signer: Some(self.signer.program().into_os_string()),
            allowed: trusted.then(|| self.signer.allowed()),
            cache: Some(self.signer.directory().join("cache")),
            environment: Vec::new(),
        }
    }

    /// `board answer <id> <answer>`, as the owner.
    fn answer(&self, id: &str, answered: Answer) {
        let owner = Asker {
            personal: Some(self.personal.workdir().to_path_buf()),
            signing: self.signing(true),
            ..Asker::default()
        };
        answer(&owner, id, answered, None, None).unwrap();
    }
}

/// `outer.due` asking for `op`, with `known` and `signing`, refused with the
/// decide's code and its effect, or admitted with the effect.
fn decided(
    scenario: &Scenario,
    op: &'static str,
    known: Option<&KnownBoards>,
    signing: Signing,
) -> Result<Option<String>, (String, Option<String>)> {
    let mut board = open(scenario);
    board.build_in(built(vec![deciding(op)]));
    let plugins = Plugins {
        known,
        signing,
        ..Plugins::default()
    };
    match board.call(&target("outer.due"), br#"{"n": 1}"#, &plugins) {
        Ok(report) => Ok(report.output["effect"].as_str().map(str::to_owned)),
        Err(error) => {
            let refusal = error.refusal();
            assert_eq!(refusal.code.as_str(), "outer::board::refused");
            let code = refusal.fields["code"].as_str().unwrap().to_owned();
            let effect = refusal.fields.get("effect").and_then(Value::as_str);
            Err((code, effect.map(str::to_owned)))
        }
    }
}

fn scenario() -> Scenario {
    board(&[("outer", true)], &["outer"], Holds::Same)
}

#[test]
fn a_built_ins_decide_asks_as_board_decide_does_and_the_owners_approval_admits_it() {
    let (scenario, owner) = (scenario(), Owner::new());
    let known = owner.known();
    let ask = || {
        decided(
            &scenario,
            "outer.override",
            Some(&known),
            owner.signing(true),
        )
    };
    let (code, effect) = ask().unwrap_err();
    assert_eq!(code, "board::settings::asked");
    let id = effect.unwrap();
    assert!(
        (owner.personal.workdir())
            .join(format!("effects/pending/{id}.json"))
            .exists()
    );
    // `board decide` under BOARD_PLUGIN asks for the same effect.
    let mut direct = Board::open(scenario.workdir(), None).unwrap();
    direct.ask_as(Asker {
        plugin: Some("outer".parse().unwrap()),
        personal: Some(owner.personal.workdir().to_path_buf()),
        signing: owner.signing(true),
        ..Asker::default()
    });
    let shown = Shown::Text(b"the body".to_vec());
    let asked = (direct.decide_op("outer.override", &"/pulls/7".parse().unwrap(), Some(&shown)))
        .unwrap_err();
    assert_eq!(asked.fields().effect.as_deref(), Some(id.as_str()));

    owner.answer(&id, Answer::Approve);
    assert_eq!(ask().unwrap(), Some(id));
}

#[test]
fn a_built_ins_decide_the_owner_refused_is_refused_and_an_approved_one_is_admitted() {
    let (scenario, owner) = (scenario(), Owner::new());
    let known = owner.known();
    let ask = |op| decided(&scenario, op, Some(&known), owner.signing(true));
    let (_, refused) = ask("outer.override").unwrap_err();
    let refused = refused.unwrap();
    owner.answer(&refused, Answer::Refuse);
    let (code, effect) = ask("outer.override").unwrap_err();
    assert_eq!(
        (code.as_str(), effect),
        ("board::settings::refused", Some(refused))
    );
    let (_, approved) = ask("outer.other").unwrap_err();
    let approved = approved.unwrap();
    owner.answer(&approved, Answer::Approve);
    assert_eq!(ask("outer.other").unwrap(), Some(approved));
}

#[test]
fn a_built_ins_decide_checks_an_approval_against_the_allowed_signers_and_without_them_it_is_unsigned()
 {
    let (scenario, owner) = (scenario(), Owner::new());
    let known = owner.known();
    let ask = |trusted| {
        decided(
            &scenario,
            "outer.override",
            Some(&known),
            owner.signing(trusted),
        )
    };
    let id = ask(true).unwrap_err().1.unwrap();
    owner.answer(&id, Answer::Approve);
    let (code, _) = ask(false).unwrap_err();
    assert_eq!(code, "board::settings::unsigned");
    assert_eq!(ask(true).unwrap(), Some(id));
}

#[test]
fn a_built_ins_decide_with_no_personal_board_or_a_malformed_op_is_refused_and_its_twin_is_asked() {
    let (scenario, owner) = (scenario(), Owner::new());
    let known = owner.known();
    let no_personal = decided(&scenario, "outer.override", None, owner.signing(true));
    assert_eq!(no_personal.unwrap_err().0, "board::settings::no_personal");
    let malformed = decided(&scenario, "override", Some(&known), owner.signing(true));
    assert_eq!(malformed.unwrap_err().0, "board::cli::usage");
    let asked = decided(
        &scenario,
        "outer.override",
        Some(&known),
        owner.signing(true),
    );
    assert_eq!(asked.unwrap_err().0, "board::settings::asked");
}

#[test]
fn a_decide_through_the_personal_board_asks_as_the_plugin_for_that_board_and_one_in_its_own_board_names_its_own()
 {
    let (scenario, owner) = (scenario(), Owner::new());
    let known = owner.known();
    let mut board = open(&scenario);
    board.build_in(built(vec![answering(
        "outer",
        Box::new(|host| {
            let path = "/pulls/7".parse().unwrap();
            let refused =
                |refusal: &Refusal| PluginError::board_refused("outer", "decide", refusal);
            let personal = host.board(&Elsewhere::Personal).map_err(|r| refused(&r))?;
            let there = personal.decide("outer.override", &path, None).unwrap_err();
            let here = host.decide("outer.override", &path, None).unwrap_err();
            Ok(json!({"count": 0, "there": there.fields["effect"], "here": here.fields["effect"]}))
        }),
    )]));
    let plugins = Plugins {
        known: Some(&known),
        signing: owner.signing(true),
        ..Plugins::default()
    };
    let output = (board.call(&target("outer.due"), br#"{"n": 1}"#, &plugins))
        .unwrap()
        .output;
    let effect = |id: &Value| {
        let file = format!("effects/pending/{}.json", id.as_str().unwrap());
        let text = std::fs::read_to_string(owner.personal.workdir().join(file)).unwrap();
        serde_json::from_str::<Value>(&text).unwrap()
    };
    let (there, here) = (effect(&output["there"]), effect(&output["here"]));
    assert_eq!(there["plugin"], json!("outer"));
    assert_eq!(here["plugin"], json!("outer"));
    let root = |scenario: &Scenario| json!(scenario.workdir().canonicalize().unwrap());
    assert_eq!(there["board"], root(&owner.personal));
    assert_eq!(here["board"], root(&scenario));
}
