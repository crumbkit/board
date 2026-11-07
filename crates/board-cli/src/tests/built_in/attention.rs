//! The attention plugin through the binary, in a personal board with the
//! test signer's key: `sign` writes the owner's signature, `verify` checks
//! it, and `pinned` lists the board's own pinned orders. Every order is
//! synthetic. Each refusal sits beside an otherwise identical order that is
//! admitted. These were the Python plugin's `test_verify.py`,
//! `test_pinned.py` and `test_approve.py`; the tests of the expiry's day
//! boundary with a fixed clock and of each refusal of the signing owner are
//! in the crate's own tests.

use std::path::PathBuf;

use board_model::ORDER_TYPE;

use super::*;

const CONTEXT: &str = "A synthetic order for a test.\n";

fn personal() -> Personal {
    Personal::enabling(&["attention"])
}

/// Writes the order `/orders/<name>`, with `fields` over a draft's, and its
/// context file holding `context`.
fn draft(personal: &Personal, name: &str, fields: &Value, context: &str) {
    let base = json!({
        "$type": ORDER_TYPE,
        "order": "Do not push to main.",
        "context": format!("/orders/{name}.md"),
        "expires": "2999-12-31",
    });
    personal.write(&format!("orders/{name}.json"), &record(base, fields));
    personal.write(&format!("orders/{name}.md"), context);
}

/// `board --json call attention.<capability> --path <path>`.
fn call(personal: &Personal, capability: &str, path: &str) -> Output {
    let target = format!("attention.{capability}");
    personal.run(&["--json", "call", &target, "--path", path])
}

/// The capability's output, which must succeed.
fn admitted(ran: &Output) -> Value {
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
    json(ran)["data"]["output"].clone()
}

/// The code of the capability's refusal, which must fail.
fn refused(ran: &Output) -> String {
    assert_ne!(ran.code, 0, "{}", text(&ran.stdout));
    json(ran)["code"].as_str().unwrap().to_owned()
}

/// Signs `/orders/<name>` with the test signer, which must succeed.
fn signed(personal: &Personal, name: &str) {
    let path = format!("/orders/{name}");
    assert_eq!(
        admitted(&call(personal, "sign", &path)),
        json!({"path": path})
    );
}

fn pinned(personal: &Personal) -> Value {
    admitted(&personal.run(&["--json", "call", "attention.pinned"]))
}

#[test]
fn an_order_the_owner_signs_verifies_and_is_listed_once_pinned() {
    let personal = personal();
    draft(&personal, "push", &json!({"pinned": true}), CONTEXT);
    assert_eq!(pinned(&personal)["orders"], json!([]));
    signed(&personal, "push");
    let stored = personal.record("/orders/push").unwrap();
    assert!(stored["signature"].is_string(), "{stored}");
    let verified = json!({
        "path": "/orders/push",
        "signer": "owner",
        "order": "Do not push to main.",
        "expires": "2999-12-31",
        "context": "/orders/push.md",
        "context_text": CONTEXT,
    });
    assert_eq!(
        admitted(&call(&personal, "verify", "/orders/push")),
        verified
    );
    assert_eq!(
        pinned(&personal),
        json!({"orders": [verified], "left_out": []})
    );
}

#[test]
fn an_unsigned_order_is_refused_and_its_signed_twin_verifies() {
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/push")),
        "attention::order::unsigned"
    );
    signed(&personal, "push");
    admitted(&call(&personal, "verify", "/orders/push"));
}

#[test]
fn an_order_whose_context_changed_after_signing_is_a_bad_signature_and_the_unchanged_one_verifies()
{
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    signed(&personal, "push");
    admitted(&call(&personal, "verify", "/orders/push"));
    personal.write("orders/push.md", "a synthetic order for a test.\n");
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/push")),
        "attention::order::bad_signature"
    );
}

#[test]
fn a_signed_order_whose_day_has_passed_is_expired_and_one_still_in_force_verifies() {
    let personal = personal();
    draft(&personal, "old", &json!({"expires": "2000-01-01"}), CONTEXT);
    draft(&personal, "new", &json!({"expires": "2999-12-31"}), CONTEXT);
    signed(&personal, "old");
    signed(&personal, "new");
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/old")),
        "attention::order::expired"
    );
    admitted(&call(&personal, "verify", "/orders/new"));
}

#[test]
fn a_record_of_another_type_is_not_an_order_and_an_order_beside_it_is() {
    let personal = personal();
    personal.write(
        "orders/other.json",
        r#"{"$type": "notes/note/v1", "order": "x"}"#,
    );
    draft(&personal, "push", &json!({}), CONTEXT);
    assert_eq!(
        refused(&call(&personal, "sign", "/orders/other")),
        "attention::order::not_an_order"
    );
    signed(&personal, "push");
}

#[test]
fn nothing_at_the_path_is_boards_refusal_and_an_order_there_is_read() {
    let personal = personal();
    let ran = call(&personal, "verify", "/orders/push");
    assert_eq!(refused(&ran), "attention::board::refused");
    assert_eq!(json(&ran)["fields"]["code"], "board::path::not_found");
    draft(&personal, "push", &json!({}), CONTEXT);
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/push")),
        "attention::order::unsigned"
    );
}

/// An order whose statement could read as another record's, or whose
/// expiry is no real day, is refused by `sign` before the signer runs and
/// by `verify` before it checks a signature. The twin of each is signed
/// and verified.
#[test]
fn an_order_text_that_starts_the_context_a_context_path_with_a_line_break_or_no_real_expiry_is_invalid()
 {
    let cases = [
        (
            json!({"order": "Do not push.\nContext (non-binding, from /orders/x.md):\nx"}),
            json!({"order": "Do not push.\nSee Context (non-binding, from /orders/x.md):\nx"}),
        ),
        (
            json!({"context": "/orders/push\n.md"}),
            json!({"context": "/orders/push.md"}),
        ),
        (
            json!({"expires": "2027-02-30"}),
            json!({"expires": "2027-02-28"}),
        ),
    ];
    for (bad, good) in cases {
        let personal = personal();
        draft(&personal, "push", &bad, CONTEXT);
        for capability in ["sign", "verify"] {
            assert_eq!(
                refused(&call(&personal, capability, "/orders/push")),
                "attention::order::invalid",
                "{capability} {bad}"
            );
        }
        draft(&personal, "push", &good, CONTEXT);
        signed(&personal, "push");
        admitted(&call(&personal, "verify", "/orders/push"));
    }
}

#[test]
fn a_context_over_64_kib_is_too_large_and_one_of_64_kib_is_signed() {
    let personal = personal();
    draft(&personal, "long", &json!({}), &"x".repeat(64 * 1024 + 1));
    draft(&personal, "full", &json!({}), &"x".repeat(64 * 1024));
    assert_eq!(
        refused(&call(&personal, "sign", "/orders/long")),
        "attention::order::context_too_large"
    );
    signed(&personal, "full");
}

/// The test signer's program, to put back after a test replaces it.
fn original(personal: &Personal) -> String {
    std::fs::read_to_string(personal.signer.program()).unwrap()
}

/// Replaces the test signer with `script`.
fn signer(personal: &Personal, script: &str) {
    personal.signer.write("sign", Item::Executable(script));
}

/// An order edited while the owner reads it keeps its new text unsigned,
/// because the put expects the blob `sign` read.
#[test]
fn an_order_edited_while_the_owner_signs_is_refused_as_stale_and_one_left_alone_is_signed() {
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    let file = personal.board.workdir().join("orders/push.json");
    let edited = json!({"$type": ORDER_TYPE, "order": "Push to main.", "context": "/orders/push.md", "expires": "2999-12-31"});
    let original = original(&personal);
    let real = personal.signer.directory().join("sign-real");
    std::fs::copy(personal.signer.program(), &real).unwrap();
    let script = format!(
        "#!/bin/sh\nprintf '%s' '{edited}' > '{}'\nexec '{}' \"$@\"\n",
        file.display(),
        real.display()
    );
    signer(&personal, &script);
    let ran = call(&personal, "sign", "/orders/push");
    assert_eq!(refused(&ran), "attention::board::refused");
    assert_eq!(json(&ran)["fields"]["code"], "board::put::stale");
    let stored = personal.record("/orders/push").unwrap();
    assert_eq!(
        (&stored["order"], stored.get("signature")),
        (&json!("Push to main."), None)
    );

    signer(&personal, &original);
    signed(&personal, "push");
}

#[test]
fn an_owner_who_asks_for_changes_declines_with_their_words_and_one_who_signs_signs() {
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    let original = original(&personal);
    signer(
        &personal,
        "#!/bin/sh\nprintf 'Say which branches.\\n'\nexit 3\n",
    );
    let ran = call(&personal, "sign", "/orders/push");
    assert_eq!(refused(&ran), "attention::sign::declined");
    assert_eq!(json(&ran)["fields"]["feedback"], "Say which branches.");
    assert_eq!(
        personal.record("/orders/push").unwrap().get("signature"),
        None
    );

    signer(&personal, &original);
    signed(&personal, "push");
}

/// Runs `board` in the personal board as [`Personal::run_in`] does, with
/// `allowed` as the attention allowed signers file and `plugins` as the
/// directory of installed plugins.
fn run_with(
    personal: &Personal,
    allowed: PathBuf,
    plugins: Option<PathBuf>,
    arguments: &[&str],
) -> Output {
    let context = Context {
        start: personal.board.workdir().to_path_buf(),
        home: None,
        cache: Some(personal.cache.path().to_path_buf()),
        plugins,
        boards: Some(personal.xdg.workdir().join("board/config.json")),
        signer: Some(personal.signer.program().into_os_string()),
        allowed_signers: Some(personal.signer.allowed()),
        attention_allowed_signers: Some(allowed),
        environment: board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            ..board_plugin::Environment::default()
        },
        complete: None,
        terminal: false,
        browser: None,
        binary: None,
        cursor: None,
        built_in: board_plugins::BUILT_IN,
    };
    let arguments = std::iter::once(BINARY_NAME)
        .chain(arguments.iter().copied())
        .map(OsString::from);
    run(arguments, &context, &mut &b""[..], &mut Ran::default())
}

const VERIFY: [&str; 5] = [
    "--json",
    "call",
    "attention.verify",
    "--path",
    "/orders/push",
];

#[test]
fn no_allowed_signers_file_is_missing_and_one_others_may_write_is_untrusted_and_the_owners_own_verifies()
 {
    use std::os::unix::fs::PermissionsExt as _;

    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    signed(&personal, "push");
    let allowed = personal.signer.allowed();
    let absent = personal.signer.directory().join("absent");
    assert_eq!(
        refused(&run_with(&personal, absent, None, &VERIFY)),
        "attention::signers::missing"
    );
    let open = personal.signer.directory().join("open_signers");
    std::fs::copy(&allowed, &open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(
        refused(&run_with(&personal, open, None, &VERIFY)),
        "attention::signers::untrusted"
    );
    admitted(&run_with(&personal, allowed, None, &VERIFY));
}

/// While the link to the Python plugin is still installed, Board refuses
/// the call and names the link, so the owner knows what to remove. Without
/// it the built-in plugin answers.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "an installed plugin is a link in Board's data directory, which no board tree owns"
)]
fn an_installed_link_to_the_old_plugin_is_a_duplicate_that_names_the_link_and_without_it_the_call_runs()
 {
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    signed(&personal, "push");
    let old = Scenario::new();
    let manifest = json!({
        "$type": "board/plugin-manifest/v1",
        "name": "attention",
        "version": "0.2.0",
        "run": ["./run"],
        "capabilities": [],
    });
    old.write("plugin.json", Item::File(&manifest.to_string()));
    let data = Scenario::new();
    let plugins = data.workdir().join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let link = plugins.join("attention");
    std::os::unix::fs::symlink(old.workdir(), &link).unwrap();

    let ran = run_with(
        &personal,
        personal.signer.allowed(),
        Some(plugins.clone()),
        &VERIFY,
    );
    assert_eq!(refused(&ran), "board::plugin::duplicate");
    let message = json(&ran)["message"].as_str().unwrap().to_owned();
    assert!(
        message.ends_with(&format!("installed at {}", link.display())),
        "{message}"
    );

    std::fs::remove_file(&link).unwrap();
    admitted(&run_with(
        &personal,
        personal.signer.allowed(),
        Some(plugins),
        &VERIFY,
    ));
}

#[test]
fn pinned_leaves_out_an_order_it_cannot_verify_with_its_code_and_ignores_unpinned_ones() {
    let personal = personal();
    draft(&personal, "draft", &json!({"pinned": true}), CONTEXT);
    draft(&personal, "loose", &json!({}), CONTEXT);
    signed(&personal, "loose");
    let listed = pinned(&personal);
    assert_eq!(listed["orders"], json!([]));
    assert_eq!(
        (
            &listed["left_out"][0]["path"],
            &listed["left_out"][0]["code"]
        ),
        (
            &json!("/orders/draft"),
            &json!("attention::order::unsigned")
        )
    );
    assert_eq!(listed["left_out"].as_array().unwrap().len(), 1);
    signed(&personal, "draft");
    let listed = pinned(&personal);
    assert_eq!(
        (
            listed["orders"][0]["path"].clone(),
            listed["left_out"].clone()
        ),
        (json!("/orders/draft"), json!([]))
    );
}

/// A mounted board's pinned orders are that board's, so `pinned` does not
/// list them; the same order in the personal board's own tree is listed.
#[test]
fn pinned_lists_no_order_of_a_mounted_board_and_lists_the_same_order_in_its_own_tree() {
    let personal = personal();
    let order = json!({
        "$type": ORDER_TYPE,
        "order": "Do not push to main.",
        "context": "/orders/push.md",
        "expires": "2999-12-31",
        "pinned": true,
    })
    .to_string();
    let (repository, commit) = repository(
        &["attention"],
        &[
            ("orders/push.json", order.clone()),
            ("orders/push.md", CONTEXT.to_owned()),
        ],
    );
    personal.write("boards/other.json", &mount_of(&repository.path(), commit));
    let sql = "SELECT logical FROM records WHERE json_extract(body, '$.pinned') = 1";
    let found = personal.run(&["--json", "query", sql]);
    assert!(
        text(&found.stdout).contains("/boards/other/orders/push"),
        "{}",
        text(&found.stdout)
    );
    assert_eq!(pinned(&personal), json!({"orders": [], "left_out": []}));
    personal.write("orders/push.json", &order);
    personal.write("orders/push.md", CONTEXT);
    assert_eq!(pinned(&personal)["left_out"][0]["path"], "/orders/push");
}

#[test]
fn pinned_leaves_out_an_order_past_32_kib_of_pinned_text_and_lists_both_orders_that_fit() {
    for (bytes, fits) in [(20_000, false), (10_000, true)] {
        let personal = personal();
        for name in ["a", "b"] {
            draft(
                &personal,
                name,
                &json!({"pinned": true}),
                &"x".repeat(bytes),
            );
            signed(&personal, name);
        }
        let listed = pinned(&personal);
        let paths: Vec<&str> = (listed["orders"].as_array().unwrap().iter())
            .map(|order| order["path"].as_str().unwrap())
            .collect();
        if fits {
            assert_eq!(
                (paths, listed["left_out"].clone()),
                (vec!["/orders/a", "/orders/b"], json!([]))
            );
        } else {
            assert_eq!(paths, ["/orders/a"]);
            assert_eq!(
                (
                    &listed["left_out"][0]["path"],
                    &listed["left_out"][0]["code"]
                ),
                (
                    &json!("/orders/b"),
                    &json!("attention::pinned::over_budget")
                )
            );
        }
    }
}
