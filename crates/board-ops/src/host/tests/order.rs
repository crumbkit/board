//! A built-in plugin signs and verifies an order through its host, with the
//! signer and the attention allowed signers file Board was given.

use board_model::{Expiry, Order, Pinned};
use board_scenario::signer::Signer;

use super::*;
use crate::effects::Signing;
use crate::{SignOrderError, VerifyOrderError};

fn order() -> Order {
    Order {
        order: "Do not push to main.".to_owned(),
        context_path: "/orders/synthetic.context.md".to_owned(),
        expires: Expiry {
            year: 2026,
            month: 12,
            day: 31,
        },
        pinned: Pinned::Yes,
        context_text: "A synthetic order for a test.\n".to_owned(),
    }
}

/// `outer`, which signs the order and answers whom its signature verifies
/// as, or the reason either was refused.
fn signing_plugin() -> Fixture {
    answering(
        "outer",
        Box::new(|host| {
            let failed = |message: String| PluginError {
                code: "outer::order::unsigned".to_owned(),
                message,
                fields: serde_json::Map::new(),
                hint: None,
            };
            let signature =
                (host.sign_order(&order())).map_err(|error| failed(error.to_string()))?;
            let signer = (host.verify_order(&order(), &signature))
                .map_err(|error| failed(error.to_string()))?;
            Ok(json!({"count": 0, "signer": signer}))
        }),
    )
}

#[test]
fn a_built_in_plugin_signs_an_order_through_its_host_and_it_verifies_as_the_owner() {
    let signer = Signer::new();
    let scenario = board(&[("outer", true)], &["outer"], Holds::Same);
    let mut board = open(&scenario);
    board.build_in(built(vec![signing_plugin()]));
    let call = |attention_allowed| {
        let plugins = Plugins {
            signing: Signing {
                signer: Some(signer.program().into_os_string()),
                cache: Some(signer.directory().join("cache")),
                attention_allowed,
                ..Signing::default()
            },
            ..Plugins::default()
        };
        board.call(&target("outer.due"), br#"{"n": 1}"#, &plugins)
    };
    let report = call(Some(signer.allowed())).unwrap();
    assert_eq!(report.output["signer"], json!("owner"));
    let refused = call(None).unwrap_err().refusal();
    assert_eq!(refused.code.as_str(), "outer::order::unsigned");
    assert!(
        refused
            .message
            .contains("no allowed signers file for orders is named"),
        "{}",
        refused.message
    );
}

/// A host that offers no signing, such as a test's, refuses both.
#[test]
fn a_host_that_offers_no_signing_refuses_to_sign_or_verify_an_order() {
    let host = Caller::at(None);
    assert!(matches!(
        host.sign_order(&order()),
        Err(SignOrderError::NotOffered)
    ));
    assert!(matches!(
        host.verify_order(&order(), "x"),
        Err(VerifyOrderError::NotOffered)
    ));
}
