#![expect(
    clippy::disallowed_methods,
    reason = "the owner's tests sign and verify orders as the effects module does"
)]
//! Signing and verifying an order, each refusal beside an admitted twin.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use board_model::{APPROVAL_HEADER, Expiry, ORDER_TYPE, Order, Pinned};
use board_plugin::CANCELLED;
use board_scenario::Item;
use board_scenario::signer::Signer;

use super::{SignOrderError, VerifyOrderError, sign_order, verify_order};
use crate::effects::Signing;

fn order() -> Order {
    Order {
        order: "Do not push to main.".to_owned(),
        context_path: "/orders/synthetic.context.md".to_owned(),
        expires: Expiry {
            year: 2026,
            month: 12,
            day: 31,
        },
        pinned: Pinned::No,
        context_text: "A synthetic order for a test.\n".to_owned(),
    }
}

/// How `signer` signs an order, and the attention allowed signers file that
/// lists its key. Board's own file is left unnamed, so an order never reads it.
fn signing(signer: &Signer) -> Signing {
    Signing {
        signer: Some(signer.program().into_os_string()),
        cache: Some(signer.directory().join("cache")),
        attention_allowed: Some(signer.allowed()),
        ..Signing::default()
    }
}

fn root(signer: &Signer) -> &Path {
    signer.directory()
}

#[test]
fn an_order_the_owner_signs_verifies_as_the_owner() {
    let signer = Signer::new();
    let signing = signing(&signer);
    let signature = sign_order(&signing, root(&signer), &order()).unwrap();
    assert!(signature.starts_with("-----BEGIN SSH"), "{signature}");
    assert_eq!(
        verify_order(&signing, &order(), &signature).unwrap(),
        "owner"
    );
}

#[test]
fn the_statement_file_is_removed_once_the_order_is_signed() {
    let signer = Signer::new();
    let signing = signing(&signer);
    sign_order(&signing, root(&signer), &order()).unwrap();
    let cache = signer.directory().join("cache");
    let left: Vec<_> = walk(&cache)
        .into_iter()
        .filter(|path| path.is_file())
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

fn walk(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut found = vec![directory.to_path_buf()];
    let mut index = 0;
    while index < found.len() {
        if found[index].is_dir() {
            for entry in std::fs::read_dir(&found[index]).unwrap() {
                found.push(entry.unwrap().path());
            }
        }
        index += 1;
    }
    found
}

/// Each change to what the owner read, once signed, is refused as a bad
/// signature, beside the unchanged order, which verifies.
#[test]
fn a_signature_is_refused_for_an_order_with_a_changed_context_byte_expiry_or_pin() {
    let signer = Signer::new();
    let signing = signing(&signer);
    let signature = signer.sign(&order().statement());
    assert_eq!(
        verify_order(&signing, &order(), &signature).unwrap(),
        "owner"
    );
    let mut context = order();
    context.context_text.replace_range(0..1, "a");
    let mut expiry = order();
    expiry.expires.day = 30;
    let mut pinned = order();
    pinned.pinned = Pinned::Yes;
    for (what, changed) in [("context", context), ("expiry", expiry), ("pinned", pinned)] {
        let refused = verify_order(&signing, &changed, &signature);
        assert!(
            matches!(&refused, Err(VerifyOrderError::Bad { file, .. }) if *file == signer.allowed()),
            "{what}: {refused:?}"
        );
    }
}

#[test]
fn a_signature_by_a_key_the_attention_file_does_not_list_is_refused() {
    let owner = Signer::new();
    let stranger = Signer::new();
    let signing = signing(&owner);
    let statement = order().statement();
    verify_order(&signing, &order(), &owner.sign(&statement)).unwrap();
    let refused = verify_order(&signing, &order(), &stranger.sign(&statement));
    assert!(
        matches!(refused, Err(VerifyOrderError::Bad { .. })),
        "{refused:?}"
    );
}

#[test]
fn an_order_signed_in_the_approval_namespace_is_refused() {
    let signer = Signer::new();
    let signing = signing(&signer);
    let statement = order().statement();
    verify_order(&signing, &order(), &signer.sign_in(ORDER_TYPE, &statement)).unwrap();
    let refused = verify_order(
        &signing,
        &order(),
        &signer.sign_in(APPROVAL_HEADER, &statement),
    );
    assert!(
        matches!(refused, Err(VerifyOrderError::Bad { .. })),
        "{refused:?}"
    );
}

#[test]
fn an_attention_signers_file_its_group_may_write_is_untrusted() {
    let signer = Signer::new();
    let signing = signing(&signer);
    let signature = signer.sign(&order().statement());
    verify_order(&signing, &order(), &signature).unwrap();
    std::fs::set_permissions(signer.allowed(), std::fs::Permissions::from_mode(0o664)).unwrap();
    let refused = verify_order(&signing, &order(), &signature);
    assert!(
        matches!(
            &refused,
            Err(VerifyOrderError::Untrusted { file, why: "its group or others may write it" })
                if *file == signer.allowed()
        ),
        "{refused:?}"
    );
}

/// An order is checked against the attention file alone: Board's own
/// allowed signers file, which lists the key, does not stand in for it.
#[test]
fn an_order_is_verified_only_against_the_attention_allowed_signers_file() {
    let signer = Signer::new();
    let signature = signer.sign(&order().statement());
    verify_order(&signing(&signer), &order(), &signature).unwrap();
    let board_only = Signing {
        allowed: Some(signer.allowed()),
        attention_allowed: None,
        ..signing(&signer)
    };
    let refused = verify_order(&board_only, &order(), &signature);
    assert!(
        matches!(refused, Err(VerifyOrderError::NoneNamed)),
        "{refused:?}"
    );
    let absent = signer.directory().join("absent");
    let missing = Signing {
        attention_allowed: Some(absent.clone()),
        ..signing(&signer)
    };
    let refused = verify_order(&missing, &order(), &signature);
    assert!(
        matches!(&refused, Err(VerifyOrderError::NoFile { file }) if *file == absent),
        "{refused:?}"
    );
}

/// A program at `name` beside the key that runs `body`.
fn program(signer: &Signer, name: &str, body: &str) -> std::ffi::OsString {
    signer.write(name, Item::Executable(&format!("#!/bin/sh\n{body}\n")));
    signer.directory().join(name).into_os_string()
}

#[test]
fn an_owner_who_asks_for_changes_or_cancels_leaves_the_order_unsigned_and_says_which() {
    let signer = Signer::new();
    sign_order(&signing(&signer), root(&signer), &order()).unwrap();
    let changes = Signing {
        signer: Some(program(&signer, "changes", "echo 'say why'\nexit 3")),
        ..signing(&signer)
    };
    let refused = sign_order(&changes, root(&signer), &order());
    assert!(
        matches!(&refused, Err(SignOrderError::Changes { feedback }) if feedback == "say why"),
        "{refused:?}"
    );
    let cancel = Signing {
        signer: Some(program(
            &signer,
            "cancel",
            &format!("echo '{CANCELLED}' >&2\nexit 1"),
        )),
        ..signing(&signer)
    };
    let refused = sign_order(&cancel, root(&signer), &order());
    assert!(
        matches!(refused, Err(SignOrderError::Cancelled)),
        "{refused:?}"
    );
}

#[test]
fn an_order_with_no_signer_or_a_failing_signer_is_unsigned() {
    let signer = Signer::new();
    sign_order(&signing(&signer), root(&signer), &order()).unwrap();
    let none = Signing {
        signer: None,
        ..signing(&signer)
    };
    let refused = sign_order(&none, root(&signer), &order());
    assert!(
        matches!(refused, Err(SignOrderError::NoSigner)),
        "{refused:?}"
    );
    let failing = Signing {
        signer: Some(program(&signer, "fail", "exit 2")),
        ..signing(&signer)
    };
    let refused = sign_order(&failing, root(&signer), &order());
    assert!(
        matches!(refused, Err(SignOrderError::Failed { .. })),
        "{refused:?}"
    );
}

/// A signer that signs the approval namespace whatever it is asked makes a
/// signature that `sign_order` checks and refuses before returning it.
#[test]
fn a_signature_the_signer_makes_in_another_namespace_is_not_returned() {
    let signer = Signer::new();
    sign_order(&signing(&signer), root(&signer), &order()).unwrap();
    let wrong = Signing {
        signer: Some(signer.program_in(APPROVAL_HEADER).into_os_string()),
        ..signing(&signer)
    };
    let refused = sign_order(&wrong, root(&signer), &order());
    assert!(
        matches!(
            refused,
            Err(SignOrderError::Unverified {
                source: VerifyOrderError::Bad { .. }
            })
        ),
        "{refused:?}"
    );
}
