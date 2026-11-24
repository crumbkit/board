#![expect(
    clippy::disallowed_methods,
    reason = "the owner's tests run the signer and the verifier"
)]
use std::ffi::OsString;
use std::path::PathBuf;

use board_scenario::Item;
use board_scenario::signer::Signer;

use std::time::Duration;

use super::{
    CANCELLED, Duplicate, LeftOutReason, LineChange, SignError, Signers, Verb, merged_signers,
    sign, signers, verify,
};

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

/// board-sign, and attention-sign before it, exit 1 for every failure and
/// say on stderr when the owner cancelled, each naming itself.
#[test]
fn an_owner_who_cancels_is_told_apart_from_a_signer_that_fails() {
    let signer = Signer::new();
    let file = statement_file(&signer, ORDER);
    for (name, line) in [
        (
            "board-sign",
            "board-sign: the owner cancelled, so nothing was signed",
        ),
        (
            "attention-sign",
            "attention-sign: the owner cancelled, so nothing was signed",
        ),
    ] {
        let cancelling = program(&signer, name, &format!("echo '{line}' >&2\nexit 1"));
        let refused = sign(&cancelling, &file, &[], Verb::Sign);
        assert!(
            matches!(refused, Err(SignError::Cancelled { .. })),
            "{name}: {refused:?}"
        );
    }
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

/// The lines of `signers` that differ from Board's file, as `(line, change)`.
fn changes(signers: &Signers) -> Vec<(usize, LineChange)> {
    (signers.changes.iter())
        .map(|changed| (changed.line, changed.change))
        .collect()
}

/// What `merged_signers` left out of the attention file, as `(line, reason)`.
fn left_out(signers: &Signers) -> Vec<(usize, LeftOutReason)> {
    (signers.left_out.iter())
        .map(|left| (left.line, left.reason))
        .collect()
}

/// A key Board's file limits to other namespaces gains the order
/// namespace when the attention line admits owner in orders; comments and
/// other keys are printed as they were.
#[test]
fn a_key_in_both_files_keeps_boards_line_and_gains_the_order_namespace() {
    let board = "# the owner's key\nowner namespaces=\"board/approve/v1,board/write/v1\" ecdsa-sha2-nistp256 AAAA one\nci ssh-ed25519 CCCC ci\n";
    let attention = "owner ecdsa-sha2-nistp256 AAAA attention\n";
    let signers = merged_signers(board, attention);
    assert_eq!(
        signers.text,
        "# the owner's key\nowner namespaces=\"board/approve/v1,board/write/v1,attention/order/v1\" ecdsa-sha2-nistp256 AAAA one\nci ssh-ed25519 CCCC ci\n"
    );
    assert_eq!(changes(&signers), [(2, LineChange::Gained)]);
    assert_eq!(signers.duplicates, []);
    assert_eq!(left_out(&signers), []);
}

/// A Board line with no `namespaces=` already admits every namespace, and
/// one that names the order namespace already admits orders: neither changes.
#[test]
fn a_board_line_that_already_admits_orders_is_printed_unchanged() {
    for board in [
        "owner ecdsa-sha2-nistp256 AAAA one\n",
        "owner namespaces=\"attention/order/v1\" ecdsa-sha2-nistp256 AAAA one\n",
    ] {
        let signers = merged_signers(board, "owner ecdsa-sha2-nistp256 AAAA\n");
        assert_eq!(signers.text, board);
        assert_eq!(changes(&signers), []);
    }
}

/// Board's line gains the order namespace only when it names `owner` and
/// every principal on it is one the attention line admits in orders, with
/// the same other options; otherwise it is left as it was and the attention line is
/// reported.
#[test]
fn a_board_line_with_other_principals_or_options_gains_nothing_and_is_reported() {
    let attention = "owner ecdsa-sha2-nistp256 AAAA\n";
    for board in [
        "owner,bob namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n",
        "owner valid-before=\"20270101\",namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n",
    ] {
        let signers = merged_signers(board, attention);
        assert_eq!(signers.text, board);
        assert_eq!(changes(&signers), []);
        assert_eq!(left_out(&signers), [(1, LeftOutReason::Differs)]);
    }
    let board = "bob namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n";
    let signers = merged_signers(board, "owner,bob ecdsa-sha2-nistp256 AAAA\n");
    assert_eq!(signers.text, board);
    assert_eq!(left_out(&signers), [(1, LeftOutReason::Differs)]);
}

/// A key only the attention file lists is added for orders alone, keeping
/// the attention line's principals and other options, though its line
/// names no namespace.
#[test]
fn a_key_only_in_the_attention_file_is_added_for_the_order_namespace_alone() {
    let board = "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA one";
    let attention = "# old\nowner ssh-ed25519 BBBB laptop\nowner ssh-ed25519 BBBB laptop\nowner valid-before=\"20200101\" ssh-ed25519 DDDD old\nowner cert-authority ssh-ed25519 EEEE ca\n";
    let signers = merged_signers(board, attention);
    assert_eq!(
        signers.text,
        "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA one\nowner namespaces=\"attention/order/v1\" ssh-ed25519 BBBB laptop\nowner valid-before=\"20200101\",namespaces=\"attention/order/v1\" ssh-ed25519 DDDD old\nowner cert-authority,namespaces=\"attention/order/v1\" ssh-ed25519 EEEE ca\n"
    );
    assert_eq!(
        changes(&signers),
        [
            (2, LineChange::Added),
            (3, LineChange::Added),
            (4, LineChange::Added)
        ]
    );
    assert_eq!(left_out(&signers), [(3, LeftOutReason::Duplicate)]);
}

/// ssh-keygen reads `!`, `*` and `?` in principals and namespaces as
/// patterns, so an attention line naming one, or one whose key Board lists
/// on a line naming one, gives nothing and Board's line prints unchanged.
#[test]
fn an_attention_or_board_line_with_patterns_gives_nothing() {
    for (board, attention) in [
        (
            "owner,bob namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n",
            "owner,bob,!bob ecdsa-sha2-nistp256 AAAA\n",
        ),
        (
            "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n",
            "owner,!owner ecdsa-sha2-nistp256 AAAA\n",
        ),
        (
            "",
            "owner namespaces=\"attention/order/v1,!attention/*\" ssh-ed25519 BBBB\n",
        ),
        ("", "own* ssh-ed25519 BBBB\n"),
        ("", "owner,b?b ssh-ed25519 BBBB\n"),
        (
            "owner,!bob namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n",
            "owner ecdsa-sha2-nistp256 AAAA\n",
        ),
        (
            "owner namespaces=\"board/*\" ecdsa-sha2-nistp256 AAAA\n",
            "owner ecdsa-sha2-nistp256 AAAA\n",
        ),
    ] {
        let signers = merged_signers(board, attention);
        assert_eq!(signers.text, board, "{attention}");
        assert_eq!(changes(&signers), []);
        assert_eq!(left_out(&signers), [(1, LeftOutReason::Pattern)]);
    }
}

/// An attention line that admits owner and more principals than Board's
/// line gives that line the order namespace, since it admits all of them.
#[test]
fn an_attention_line_with_more_principals_than_boards_line_gives_it_orders() {
    let board = "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n";
    let signers = merged_signers(board, "owner,bob ecdsa-sha2-nistp256 AAAA\n");
    assert_eq!(
        signers.text,
        "owner namespaces=\"board/approve/v1,attention/order/v1\" ecdsa-sha2-nistp256 AAAA\n"
    );
    assert_eq!(changes(&signers), [(1, LineChange::Gained)]);
    assert_eq!(left_out(&signers), []);
}

/// Options are compared as written, so the same options in another order
/// neither merge two of Board's lines nor let an attention line give one
/// the order namespace.
#[test]
fn the_same_options_in_another_order_do_not_merge() {
    let board = "owner cert-authority,valid-before=\"20270101\",namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\nowner valid-before=\"20270101\",cert-authority,namespaces=\"board/write/v1\" ecdsa-sha2-nistp256 AAAA\n";
    let signers = merged_signers(board, "");
    assert_eq!(signers.text, board);
    assert_eq!(changes(&signers), []);
    assert!(!signers.duplicates[0].merged);

    let board = "owner cert-authority,valid-before=\"20270101\",namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n";
    let attention = "owner valid-before=\"20270101\",cert-authority ecdsa-sha2-nistp256 AAAA\n";
    let signers = merged_signers(board, attention);
    assert_eq!(signers.text, board);
    assert_eq!(left_out(&signers), [(1, LeftOutReason::Differs)]);
}

/// A `namespaces=` value without quotes, or two `namespaces=` options on one
/// line, is refused by ssh-keygen, so such a line cannot be read: Board's
/// prints as it was and the attention file's is left out as unreadable, as
/// is a line with no key.
#[test]
fn an_unquoted_or_doubled_namespaces_or_a_line_with_no_key_is_unreadable() {
    let attention = "owner namespaces=attention/order/v1 ssh-ed25519 BBBB\nowner namespaces=\"git\",namespaces=\"attention/order/v1\" ssh-ed25519 CCCC\nowner ssh-ed25519\n\n# a comment\n";
    let signers = merged_signers("", attention);
    assert_eq!(signers.text, "");
    assert_eq!(
        left_out(&signers),
        [
            (1, LeftOutReason::Unreadable),
            (2, LeftOutReason::Unreadable),
            (3, LeftOutReason::Unreadable),
        ]
    );
    assert!(signers.left_out.iter().all(|left| left.key.is_none()));

    let board = "owner namespaces=board/approve/v1 ecdsa-sha2-nistp256 AAAA\nowner namespaces=\"git\",namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 BBBB\n";
    let signers = merged_signers(board, "");
    assert_eq!(signers.text, board);
    assert_eq!(changes(&signers), []);
}

/// A later attention line for a key an earlier line already gave is left
/// out as a duplicate, with its key, though its options differ.
#[test]
fn a_later_attention_line_for_a_key_already_given_is_a_duplicate() {
    let attention =
        "owner valid-before=\"20200101\" ssh-ed25519 BBBB old\nowner ssh-ed25519 BBBB new\n";
    let signers = merged_signers("", attention);
    assert_eq!(
        signers.text,
        "owner valid-before=\"20200101\",namespaces=\"attention/order/v1\" ssh-ed25519 BBBB old\n"
    );
    assert_eq!(left_out(&signers), [(2, LeftOutReason::Duplicate)]);
    assert_eq!(signers.left_out[0].key.as_deref(), Some("ssh-ed25519 BBBB"));
}

/// An attention line that does not admit owner in orders adds nothing and
/// gives Board's line nothing; it is reported by its line number.
#[test]
fn an_attention_line_for_another_principal_or_other_namespaces_is_left_out() {
    let board = "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\n";
    let attention = "bob ssh-ed25519 BBBB\nowner namespaces=\"git\" ssh-ed25519 CCCC\nbob ecdsa-sha2-nistp256 AAAA\nowner namespaces=\"git\" ecdsa-sha2-nistp256 AAAA\n";
    let signers = merged_signers(board, attention);
    assert_eq!(signers.text, board);
    assert_eq!(changes(&signers), []);
    assert_eq!(
        left_out(&signers),
        [
            (1, LeftOutReason::NotOwner),
            (2, LeftOutReason::NotOrders),
            (3, LeftOutReason::NotOwner),
            (4, LeftOutReason::NotOrders),
        ]
    );
}

/// Two lines of Board's file with one key, the same principals and the same
/// other options print as one line holding both lines' namespaces, or none
/// when one line names none; both are reported by their line numbers.
#[test]
fn two_board_lines_with_one_key_and_the_same_principals_and_options_print_as_one() {
    let board = "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA one\nci ssh-ed25519 CCCC ci\nowner namespaces=\"board/write/v1\" ecdsa-sha2-nistp256 AAAA two\n";
    let signers = merged_signers(board, "");
    assert_eq!(
        signers.text,
        "owner namespaces=\"board/approve/v1,board/write/v1\" ecdsa-sha2-nistp256 AAAA one\nci ssh-ed25519 CCCC ci\n"
    );
    assert_eq!(changes(&signers), [(1, LineChange::Merged)]);
    assert_eq!(
        signers.duplicates,
        [Duplicate {
            key: "ecdsa-sha2-nistp256 AAAA".to_owned(),
            lines: vec![1, 3],
            merged: true,
        }]
    );
    let unrestricted = "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\nowner ecdsa-sha2-nistp256 AAAA\n";
    let signers = merged_signers(unrestricted, "");
    assert_eq!(signers.text, "owner ecdsa-sha2-nistp256 AAAA\n");
    assert_eq!(changes(&signers), [(1, LineChange::Merged)]);
}

/// Two lines with one key whose principals or other options differ would
/// admit more as one line, so both print unchanged and are reported.
#[test]
fn two_board_lines_with_one_key_and_other_principals_or_options_print_unchanged() {
    for board in [
        "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\nbob namespaces=\"git\" ecdsa-sha2-nistp256 AAAA\n",
        "owner namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\nbob ecdsa-sha2-nistp256 AAAA\n",
        "owner valid-before=\"20270101\",namespaces=\"board/approve/v1\" ecdsa-sha2-nistp256 AAAA\nowner namespaces=\"board/write/v1\" ecdsa-sha2-nistp256 AAAA\n",
    ] {
        let signers = merged_signers(board, "");
        assert_eq!(signers.text, board);
        assert_eq!(changes(&signers), []);
        assert_eq!(
            signers.duplicates,
            [Duplicate {
                key: "ecdsa-sha2-nistp256 AAAA".to_owned(),
                lines: vec![1, 2],
                merged: false,
            }]
        );
    }
}

/// The printed lines admit an order the attention file alone admitted, and
/// reading the files writes neither. A missing file reads as empty; a Board
/// file others may write is refused.
#[test]
fn the_merged_lines_admit_an_order_and_reading_the_files_writes_neither() {
    let signer = Signer::new();
    let public = std::fs::read_to_string(signer.directory().join("key.pub")).unwrap();
    signer.write(
        "board_signers",
        Item::File(&format!("owner namespaces=\"board/approve/v1\" {public}")),
    );
    let board = signer.directory().join("board_signers");
    let signature = signer.sign(ORDER);
    assert!(matches!(
        verify(&board, ORDER, &signature),
        Err(SignError::Bad { .. })
    ));
    let before = (
        std::fs::read(&board).unwrap(),
        std::fs::read(signer.allowed()).unwrap(),
    );
    let read = signers(&board, Some(&signer.allowed())).unwrap();
    assert_eq!(read.file, board);
    assert_eq!(
        before,
        (
            std::fs::read(&board).unwrap(),
            std::fs::read(signer.allowed()).unwrap()
        )
    );
    signer.write("merged", Item::File(&read.text));
    verify(&signer.directory().join("merged"), ORDER, &signature).unwrap();
    verify(
        &signer.directory().join("merged"),
        STATEMENT,
        &signer.sign(STATEMENT),
    )
    .unwrap();

    let absent = signer.directory().join("absent");
    assert_eq!(signers(&absent, Some(&absent)).unwrap().text, "");
    assert_eq!(signers(&absent, None).unwrap().text, "");
    shared(&board);
    let refused = signers(&board, Some(&signer.allowed()));
    assert!(
        matches!(&refused, Err(SignError::Untrusted { file, .. }) if *file == board),
        "{refused:?}"
    );
}

/// ssh-keygen ends a principals word at a closing double quote and reads
/// what follows as options, so a principals word holding a quote can hide
/// options such as `namespaces=`: the line is unreadable and gives nothing.
#[test]
fn a_principals_word_holding_a_quote_is_unreadable_and_gives_nothing() {
    let board = "owner namespaces=\"board/approve/v1\" ssh-ed25519 KKKK\n";
    for attention in [
        "owner,\"x\"namespaces=\"git\" ssh-ed25519 KKKK\n",
        "owner,\"x\"cert-authority ssh-ed25519 KKKK\n",
        "\"owner\" ssh-ed25519 KKKK\n",
    ] {
        let signers = merged_signers(board, attention);
        assert_eq!(signers.text, board, "{attention}");
        assert_eq!(changes(&signers), []);
        assert_eq!(left_out(&signers), [(1, LeftOutReason::Unreadable)]);
    }
    let board = "owner,\"x\"namespaces=\"board/approve/v1\" ssh-ed25519 KKKK\nowner namespaces=\"git\" ssh-ed25519 KKKK\n";
    let signers = merged_signers(board, "");
    assert_eq!(signers.text, board);
    assert_eq!(changes(&signers), []);
}

/// ssh-keygen splits a line only at spaces and tabs, so a line holding any
/// other whitespace or a control character is unreadable on both sides,
/// while a tab still separates words.
#[test]
fn a_line_with_whitespace_other_than_space_or_tab_is_unreadable() {
    let attention = "owner ssh-ed25519 BBBB\u{a0}laptop\n\u{a0}owner ssh-ed25519 CCCC\nowner\u{b}ssh-ed25519 DDDD\nowner ssh-ed25519\u{c}EEEE\n\u{2003}\nowner\tssh-ed25519\tFFFF\n";
    let signers = merged_signers("", attention);
    assert_eq!(
        signers.text,
        "owner namespaces=\"attention/order/v1\" ssh-ed25519 FFFF\n"
    );
    assert_eq!(
        left_out(&signers),
        [
            (1, LeftOutReason::Unreadable),
            (2, LeftOutReason::Unreadable),
            (3, LeftOutReason::Unreadable),
            (4, LeftOutReason::Unreadable),
            (5, LeftOutReason::Unreadable),
        ]
    );

    let board = "owner namespaces=\"board/approve/v1\" ssh-ed25519 BBBB\u{a0}laptop\nowner namespaces=\"git\" ssh-ed25519 BBBB\n";
    let signers = merged_signers(board, "");
    assert_eq!(signers.text, board);
    assert_eq!(changes(&signers), []);
}

/// ssh-keygen refuses options with an empty piece, such as a trailing
/// comma, and reads a backslash before a quote as an escape that leaves the
/// quote open, so either makes a line unreadable on both sides.
#[test]
fn an_empty_option_or_a_backslash_in_the_options_is_unreadable() {
    let attention = "owner namespaces=\"attention/order/v1\", ssh-ed25519 BBBB\nowner ,namespaces=\"attention/order/v1\" ssh-ed25519 CCCC\nowner cert-authority,,namespaces=\"attention/order/v1\" ssh-ed25519 DDDD\nowner namespaces=\"attention/order/v1,git\\\" ssh-ed25519 EEEE\n";
    let signers = merged_signers("", attention);
    assert_eq!(signers.text, "");
    assert_eq!(
        left_out(&signers),
        [
            (1, LeftOutReason::Unreadable),
            (2, LeftOutReason::Unreadable),
            (3, LeftOutReason::Unreadable),
            (4, LeftOutReason::Unreadable),
        ]
    );

    for board in [
        "owner namespaces=\"git\\\" ecdsa-sha2-nistp256 AAAA\n",
        "owner namespaces=\"git\", ecdsa-sha2-nistp256 AAAA\n",
    ] {
        let signers = merged_signers(board, "owner ecdsa-sha2-nistp256 AAAA\n");
        assert!(signers.text.starts_with(board), "{}", signers.text);
        assert_eq!(changes(&signers), [(2, LineChange::Added)]);
    }
}

/// ssh-keygen matches no name against a list holding an element of 1023
/// bytes or more, so such an element counts as a pattern; one of 1022
/// bytes does not.
#[test]
fn a_list_element_of_1023_bytes_or_more_is_a_pattern() {
    let long = "b".repeat(1023);
    let attention = format!(
        "owner,{long} ssh-ed25519 BBBB\nowner namespaces=\"attention/order/v1,{long}\" ssh-ed25519 CCCC\nowner,{} ssh-ed25519 DDDD\n",
        &long[1..]
    );
    let signers = merged_signers("", &attention);
    assert_eq!(
        left_out(&signers),
        [(1, LeftOutReason::Pattern), (2, LeftOutReason::Pattern)]
    );
    assert_eq!(changes(&signers), [(1, LineChange::Added)]);

    let board = format!("owner namespaces=\"board/approve/v1,{long}\" ssh-ed25519 BBBB\n");
    let signers = merged_signers(&board, "owner ssh-ed25519 BBBB\n");
    assert_eq!(signers.text, board);
    assert_eq!(left_out(&signers), [(1, LeftOutReason::Pattern)]);
}

/// An attention line for a key Board's lines already admit in orders gives
/// nothing, and it is reported as already admitted.
#[test]
fn an_attention_line_for_a_key_board_already_admits_in_orders_is_reported() {
    let board = "owner namespaces=\"attention/order/v1\" ecdsa-sha2-nistp256 AAAA\nowner ssh-ed25519 BBBB\n";
    let attention = "owner ecdsa-sha2-nistp256 AAAA\nowner ssh-ed25519 BBBB\n";
    let signers = merged_signers(board, attention);
    assert_eq!(signers.text, board);
    assert_eq!(changes(&signers), []);
    assert_eq!(
        left_out(&signers),
        [
            (1, LeftOutReason::AlreadyAdmitted),
            (2, LeftOutReason::AlreadyAdmitted)
        ]
    );
    assert_eq!(
        signers.left_out[0].key.as_deref(),
        Some("ecdsa-sha2-nistp256 AAAA")
    );
}
