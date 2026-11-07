#![expect(
    clippy::disallowed_methods,
    reason = "the owner's tests run the signer and the verifier"
)]
use std::ffi::OsString;
use std::path::PathBuf;

use board_scenario::Item;
use board_scenario::signer::Signer;

use std::time::Duration;

use super::{CANCELLED, SignError, Verb, sign, verify};

const STATEMENT: &str = "board/approve/v1\neffect: 1\n";

fn statement_file(signer: &Signer, text: &str) -> PathBuf {
    signer.write("to-sign", Item::File(text));
    signer.directory().join("to-sign")
}

fn program(signer: &Signer, name: &str, body: &str) -> OsString {
    signer.write(name, Item::Executable(&format!("#!/bin/sh\n{body}\n")));
    signer.directory().join(name).into_os_string()
}

#[test]
fn a_signature_the_signer_makes_verifies_for_the_statement_it_signed() {
    let signer = Signer::new();
    let file = statement_file(&signer, STATEMENT);
    let signature = sign(
        &signer.program().into_os_string(),
        &file,
        &[],
        Verb::Approve,
    )
    .unwrap();
    verify(&signer.allowed(), STATEMENT, &signature).unwrap();
}

#[test]
fn a_signature_of_another_statement_is_refused() {
    let signer = Signer::new();
    let signature = signer.sign(STATEMENT);
    verify(&signer.allowed(), STATEMENT, &signature).unwrap();
    let refused = verify(
        &signer.allowed(),
        "board/approve/v1\neffect: 2\n",
        &signature,
    );
    assert!(matches!(refused, Err(SignError::Bad { .. })), "{refused:?}");
}

const WRITE: &str = "board/write/v1\nop: put\npath: /preferences/r\n";

#[test]
fn a_rule_change_verifies_only_when_signed_in_the_write_namespace() {
    let signer = Signer::new();
    verify(&signer.allowed(), WRITE, &signer.sign(WRITE)).unwrap();
    verify(
        &signer.allowed(),
        WRITE,
        &signer.sign_in("board/write/v1", WRITE),
    )
    .unwrap();
    let refused = verify(
        &signer.allowed(),
        WRITE,
        &signer.sign_in("board/approve/v1", WRITE),
    );
    assert!(matches!(refused, Err(SignError::Bad { .. })), "{refused:?}");
}

#[test]
fn an_approval_verifies_only_when_signed_in_the_approval_namespace() {
    let signer = Signer::new();
    let approval = signer.sign_in("board/approve/v1", STATEMENT);
    verify(&signer.allowed(), STATEMENT, &approval).unwrap();
    let refused = verify(
        &signer.allowed(),
        STATEMENT,
        &signer.sign_in("board/write/v1", STATEMENT),
    );
    assert!(matches!(refused, Err(SignError::Bad { .. })), "{refused:?}");
}

#[test]
fn a_key_limited_to_approvals_cannot_sign_a_rule_change() {
    let signer = Signer::new();
    let public = std::fs::read_to_string(signer.directory().join("key.pub")).unwrap();
    signer.write(
        "approvals_only",
        Item::File(&format!("owner namespaces=\"board/approve/v1\" {public}")),
    );
    let limited = signer.directory().join("approvals_only");
    verify(&limited, STATEMENT, &signer.sign(STATEMENT)).unwrap();
    let refused = verify(&limited, WRITE, &signer.sign(WRITE));
    assert!(matches!(refused, Err(SignError::Bad { .. })), "{refused:?}");
}

#[test]
fn a_signature_by_a_key_the_allowed_file_does_not_list_is_refused() {
    let owner = Signer::new();
    let stranger = Signer::new();
    verify(&owner.allowed(), STATEMENT, &owner.sign(STATEMENT)).unwrap();
    let refused = verify(&owner.allowed(), STATEMENT, &stranger.sign(STATEMENT));
    assert!(matches!(refused, Err(SignError::Bad { .. })), "{refused:?}");
}

#[test]
fn verifying_without_an_allowed_signers_file_is_refused() {
    let signer = Signer::new();
    let signature = signer.sign(STATEMENT);
    verify(&signer.allowed(), STATEMENT, &signature).unwrap();
    let missing = signer.directory().join("absent");
    let refused = verify(&missing, STATEMENT, &signature);
    assert!(matches!(refused, Err(SignError::NoSigners { file }) if file == missing));
}

#[test]
fn text_that_is_not_a_signature_is_refused_by_the_verifier() {
    let signer = Signer::new();
    let refused = verify(&signer.allowed(), STATEMENT, "EOF\n$(touch x)");
    assert!(matches!(refused, Err(SignError::Bad { .. })), "{refused:?}");
}

#[test]
fn a_signer_that_cannot_be_run_is_reported() {
    let signer = Signer::new();
    let file = statement_file(&signer, STATEMENT);
    signer.write("plain", Item::File("#!/bin/sh\n"));
    let plain = signer.directory().join("plain").into_os_string();
    let refused = sign(&plain, &file, &[], Verb::Approve);
    assert!(
        matches!(refused, Err(SignError::NotStarted { .. })),
        "{refused:?}"
    );
    let missing = signer.directory().join("no-such-signer").into_os_string();
    let refused = sign(&missing, &file, &[], Verb::Approve);
    assert!(
        matches!(
            refused,
            Err(SignError::Untrusted {
                why: "it cannot be read",
                ..
            })
        ),
        "{refused:?}"
    );
}

/// Lets the file's group and others write it.
fn shared(file: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o775)).unwrap();
}

#[test]
fn a_signer_or_allowed_signers_file_that_others_may_write_is_untrusted() {
    let signer = Signer::new();
    let file = statement_file(&signer, STATEMENT);
    let signature = sign(
        &signer.program().into_os_string(),
        &file,
        &[],
        Verb::Approve,
    )
    .unwrap();
    verify(&signer.allowed(), STATEMENT, &signature).unwrap();
    let untrusted = |refused: Result<_, SignError>| {
        matches!(
            refused,
            Err(SignError::Untrusted {
                why: "its group or others may write it",
                ..
            })
        )
    };
    shared(&signer.allowed());
    assert!(untrusted(verify(&signer.allowed(), STATEMENT, &signature)));
    shared(&signer.program());
    assert!(untrusted(
        sign(
            &signer.program().into_os_string(),
            &file,
            &[],
            Verb::Approve
        )
        .map(|_| ())
    ));
}

#[test]
fn the_signer_is_asked_to_approve_the_statement_file() {
    let signer = Signer::new();
    let file = statement_file(&signer, STATEMENT);
    let other = program(&signer, "other", "[ \"$1\" = sign ] || exit 1");
    let refused = sign(&other, &file, &[], Verb::Approve);
    assert!(
        matches!(&refused, Err(SignError::Failed { status, .. }) if status == "1"),
        "{refused:?}"
    );
    let approving = program(
        &signer,
        "approving",
        &format!(
            "[ \"$1\" = approve ] && [ \"$2\" = '{}' ] || exit 1\nexec '{}' \"$@\"",
            file.display(),
            signer.program().display()
        ),
    );
    let signature = sign(&approving, &file, &[], Verb::Approve).unwrap();
    verify(&signer.allowed(), STATEMENT, &signature).unwrap();
}

#[test]
fn an_owner_who_asks_for_changes_declines_with_their_feedback() {
    let signer = Signer::new();
    let file = statement_file(&signer, STATEMENT);
    let declining = program(&signer, "decline", "echo 'not this one'\nexit 3");
    let refused = sign(&declining, &file, &[], Verb::Approve);
    assert!(
        matches!(&refused, Err(SignError::Declined { feedback }) if feedback == "not this one"),
        "{refused:?}"
    );
}

#[test]
fn a_signer_that_cancels_or_prints_no_signature_fails() {
    let signer = Signer::new();
    let file = statement_file(&signer, STATEMENT);
    let cancelling = program(&signer, "cancel", "echo cancelled >&2\nexit 1");
    let refused = sign(&cancelling, &file, &[], Verb::Approve);
    assert!(
        matches!(&refused, Err(SignError::Failed { status, stderr, .. }) if status == "1" && stderr == "cancelled"),
        "{refused:?}"
    );
    let chatty = program(&signer, "chatty", "echo hello");
    let refused = sign(&chatty, &file, &[], Verb::Approve);
    assert!(
        matches!(refused, Err(SignError::NotASignature { .. })),
        "{refused:?}"
    );
}

#[test]
fn the_signer_sees_only_the_environment_it_is_given() {
    let signer = Signer::new();
    let file = statement_file(&signer, STATEMENT);
    let echo = program(
        &signer,
        "echo",
        "[ \"$GIVEN\" = yes ] && [ -z \"$HIDDEN\" ] || exit 1\nexec \"$SIGNER\" \"$@\"",
    );
    let environment = [
        ("GIVEN", OsString::from("yes")),
        ("SIGNER", signer.program().into_os_string()),
    ];
    let signature = sign(&echo, &file, &environment, Verb::Approve).unwrap();
    verify(&signer.allowed(), STATEMENT, &signature).unwrap();
}

const ORDER: &str = "attention/order/v1\nexpires: 2026-12-31\npinned: no\n\nOrder (binding):\nx\n\nContext (non-binding, from /c.md):\n";

#[test]
fn an_order_is_signed_under_sign_and_verifies_in_its_own_namespace() {
    let signer = Signer::new();
    let file = statement_file(&signer, ORDER);
    let signature = sign(&signer.program().into_os_string(), &file, &[], Verb::Sign).unwrap();
    verify(&signer.allowed(), ORDER, &signature).unwrap();
    let refused = verify(
        &signer.allowed(),
        ORDER,
        &signer.sign_in("board/approve/v1", ORDER),
    );
    assert!(matches!(refused, Err(SignError::Bad { .. })), "{refused:?}");
}

/// attention-sign signs an approval only under `approve` and an order only
/// under `sign`, and so does the test signer.
#[test]
fn the_signer_refuses_an_approval_under_sign_and_an_order_under_approve() {
    let signer = Signer::new();
    let program = signer.program().into_os_string();
    let refused = |result: Result<String, SignError>| matches!(&result, Err(SignError::Failed { status, .. }) if status == "1");
    let approval = statement_file(&signer, STATEMENT);
    sign(&program, &approval, &[], Verb::Approve).unwrap();
    assert!(refused(sign(&program, &approval, &[], Verb::Sign)));
    let order = statement_file(&signer, ORDER);
    sign(&program, &order, &[], Verb::Sign).unwrap();
    assert!(refused(sign(&program, &order, &[], Verb::Approve)));
}

#[test]
fn the_signer_is_asked_to_sign_an_order_file_with_sign() {
    let signer = Signer::new();
    let file = statement_file(&signer, ORDER);
    let signing = program(
        &signer,
        "signing",
        &format!(
            "[ \"$1\" = sign ] && [ \"$2\" = '{}' ] || exit 1\nexec '{}' \"$@\"",
            file.display(),
            signer.program().display()
        ),
    );
    let signature = sign(&signing, &file, &[], Verb::Sign).unwrap();
    verify(&signer.allowed(), ORDER, &signature).unwrap();
    let refused = sign(&signing, &file, &[], Verb::Approve);
    assert!(
        matches!(&refused, Err(SignError::Failed { status, .. }) if status == "1"),
        "{refused:?}"
    );
}

/// The owner has 20 seconds to approve, which fits the effects mod's 30 s
/// limit, and 10 minutes to read an order's context of up to 64 KiB.
#[test]
fn an_approval_has_twenty_seconds_and_an_order_ten_minutes() {
    assert_eq!(Verb::Approve.time(), Duration::from_secs(20));
    assert_eq!(Verb::Sign.time(), Duration::from_secs(600));
}

/// attention-sign exits 1 for every failure, and says on stderr when the
/// owner cancelled.
#[test]
fn an_owner_who_cancels_is_told_apart_from_a_signer_that_fails() {
    let signer = Signer::new();
    let file = statement_file(&signer, ORDER);
    let cancelling = program(
        &signer,
        "cancel",
        "echo 'attention-sign: the owner cancelled, so nothing was signed' >&2\nexit 1",
    );
    let refused = sign(&cancelling, &file, &[], Verb::Sign);
    assert!(
        matches!(refused, Err(SignError::Cancelled { .. })),
        "{refused:?}"
    );
    let failing = program(
        &signer,
        "fail",
        "echo 'attention-sign: the statement is not UTF-8' >&2\nexit 1",
    );
    let refused = sign(&failing, &file, &[], Verb::Sign);
    assert!(
        matches!(&refused, Err(SignError::Failed { status, .. }) if status == "1"),
        "{refused:?}"
    );
}

/// `AppKit` may write its own warnings to stderr before attention-sign's
/// cancel line, so the cancel line is the last line, not the only one.
#[test]
fn a_cancel_line_after_other_stderr_lines_is_a_cancel_and_one_followed_or_changed_is_a_failure() {
    let signer = Signer::new();
    let file = statement_file(&signer, ORDER);
    let after = program(
        &signer,
        "after",
        &format!(
            "echo 'AppKit: a warning' >&2\necho 'another' >&2\necho '{CANCELLED}' >&2\necho >&2\nexit 1"
        ),
    );
    let refused = sign(&after, &file, &[], Verb::Sign);
    assert!(
        matches!(refused, Err(SignError::Cancelled { .. })),
        "{refused:?}"
    );
    let followed = program(
        &signer,
        "followed",
        &format!("echo '{CANCELLED}' >&2\necho 'AppKit: a warning' >&2\nexit 1"),
    );
    let changed = program(
        &signer,
        "changed",
        &format!("echo 'AppKit: a warning' >&2\necho '{CANCELLED}.' >&2\nexit 1"),
    );
    for failing in [followed, changed] {
        let refused = sign(&failing, &file, &[], Verb::Sign);
        assert!(
            matches!(&refused, Err(SignError::Failed { status, .. }) if status == "1"),
            "{refused:?}"
        );
    }
}
