//! The orders plugin, named attention before, through the binary, in a personal board with the
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
    Personal::enabling(&["orders"])
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

/// `board --json call orders.<capability> --path <path>`.
fn call(personal: &Personal, capability: &str, path: &str) -> Output {
    let target = format!("orders.{capability}");
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
    admitted(&personal.run(&["--json", "call", "orders.pinned"]))
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
        "approval": "signed by owner",
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
        json!({
            "signing": "Signing is on: orders, effects and owner's writes need Touch ID.",
            "orders": [verified],
            "left_out": []
        })
    );
}

#[test]
fn an_unsigned_order_is_refused_and_its_signed_twin_verifies() {
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/push")),
        "orders::order::unsigned"
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
        "orders::order::bad_signature"
    );
}

/// An order the test signer signed although its day has passed, which
/// `sign` would refuse: the statement of the same order signed for 2999,
/// with its expiry line moved to 2000-01-01, signed with the test key.
fn signed_expired(personal: &Personal, name: &str) {
    draft(personal, name, &json!({}), CONTEXT);
    let original = original(personal);
    let real = personal.signer.directory().join("sign-real");
    std::fs::copy(personal.signer.program(), &real).unwrap();
    let statement = personal.signer.directory().join("statement");
    let script = format!(
        "#!/bin/sh\ncp \"$2\" '{}'\nexec '{}' \"$@\"\n",
        statement.display(),
        real.display()
    );
    signer(personal, &script);
    signed(personal, name);
    signer(personal, &original);
    let statement = std::fs::read_to_string(statement).unwrap();
    let expired = statement.replace("expires: 2999-12-31", "expires: 2000-01-01");
    assert_ne!(expired, statement, "the statement names its expiry");
    let path = format!("/orders/{name}");
    let mut stored = personal.record(&path).unwrap();
    stored["expires"] = json!("2000-01-01");
    stored["signature"] = json!(personal.signer.sign(&expired));
    personal.write(&format!("orders/{name}.json"), &stored.to_string());
}

#[test]
fn an_order_whose_day_has_passed_is_refused_as_expired_by_sign_and_verify_and_one_still_in_force_is_admitted()
 {
    let personal = personal();
    draft(
        &personal,
        "late",
        &json!({"expires": "2000-01-01"}),
        CONTEXT,
    );
    let ran = call(&personal, "sign", "/orders/late");
    assert_eq!(refused(&ran), "orders::order::expired");
    assert_eq!(
        personal.record("/orders/late").unwrap().get("signature"),
        None
    );
    draft(&personal, "new", &json!({"expires": "2999-12-31"}), CONTEXT);
    signed(&personal, "new");

    signed_expired(&personal, "old");
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/old")),
        "orders::order::expired"
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
        "orders::order::not_an_order"
    );
    signed(&personal, "push");
}

#[test]
fn nothing_at_the_path_is_boards_refusal_and_an_order_there_is_read() {
    let personal = personal();
    let ran = call(&personal, "verify", "/orders/push");
    assert_eq!(refused(&ran), "orders::board::refused");
    assert_eq!(json(&ran)["fields"]["code"], "board::path::not_found");
    draft(&personal, "push", &json!({}), CONTEXT);
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/push")),
        "orders::order::unsigned"
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
                "orders::order::invalid",
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
        "orders::order::context_too_large"
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
    assert_eq!(refused(&ran), "orders::board::refused");
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
    assert_eq!(refused(&ran), "orders::sign::declined");
    assert_eq!(json(&ran)["fields"]["feedback"], "Say which branches.");
    assert_eq!(
        personal.record("/orders/push").unwrap().get("signature"),
        None
    );

    signer(&personal, &original);
    signed(&personal, "push");
}

/// Runs `board` in the personal board as [`Personal::run_in`] does, with
/// `board` as Board's own allowed signers file, `attention` as the
/// attention file and `plugins` as the directory of installed plugins.
fn run_with(
    personal: &Personal,
    files: (PathBuf, PathBuf),
    plugins: Option<PathBuf>,
    arguments: &[&str],
) -> Output {
    run_reading(personal, files, plugins, arguments, "")
}

/// [`run_with`], with `stdin` as the standard input.
fn run_reading(
    personal: &Personal,
    (board, attention): (PathBuf, PathBuf),
    plugins: Option<PathBuf>,
    arguments: &[&str],
    stdin: &str,
) -> Output {
    let context = Context {
        start: personal.board.workdir().to_path_buf(),
        home: None,
        cache: Some(personal.cache.path().to_path_buf()),
        plugins,
        boards: Some(personal.xdg.workdir().join("board/config.json")),
        signer: Some(personal.signer.program().into_os_string()),
        allowed_signers: Some(board),
        attention_allowed_signers: Some(attention),
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
    run(
        arguments,
        &context,
        &mut stdin.as_bytes(),
        &mut Ran::default(),
    )
}

const VERIFY: [&str; 5] = ["--json", "call", "orders.verify", "--path", "/orders/push"];

/// No allowed signers file is missing; a Board file others may write is
/// untrusted, even while the attention file admits the key; the owner's
/// own Board file verifies.
#[test]
fn no_allowed_signers_file_is_missing_and_a_board_file_others_may_write_is_untrusted_and_the_owners_own_verifies()
 {
    use std::os::unix::fs::PermissionsExt as _;

    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    signed(&personal, "push");
    let allowed = personal.signer.allowed();
    let absent = personal.signer.directory().join("absent");
    assert_eq!(
        refused(&run_with(
            &personal,
            (absent.clone(), absent.clone()),
            None,
            &VERIFY
        )),
        "orders::signers::missing"
    );
    let open = personal.signer.directory().join("open_signers");
    std::fs::copy(&allowed, &open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(
        refused(&run_with(&personal, (open, allowed.clone()), None, &VERIFY)),
        "orders::signers::untrusted"
    );
    admitted(&run_with(&personal, (allowed, absent), None, &VERIFY));
}

/// The lines of `output`'s stderr that warn with `code`.
fn warned(output: &Output, code: &str) -> Vec<String> {
    let prefix = format!("warning: {code}: ");
    (text(&output.stderr).lines())
        .filter_map(|line| line.strip_prefix(prefix.as_str()).map(str::to_owned))
        .collect()
}

/// An order whose key only the attention file admits verifies, and the
/// call warns once that Board stops reading that file in its next release;
/// one Board's own file admits warns of nothing.
#[test]
fn an_order_only_the_attention_file_admits_verifies_with_a_legacy_warning() {
    let personal = personal();
    draft(&personal, "push", &json!({"pinned": true}), CONTEXT);
    draft(&personal, "main", &json!({"pinned": true}), CONTEXT);
    signed(&personal, "push");
    signed(&personal, "main");
    let allowed = personal.signer.allowed();
    let stranger = board_scenario::signer::Signer::new();
    let ran = run_with(
        &personal,
        (stranger.allowed(), allowed.clone()),
        None,
        &VERIFY,
    );
    assert_eq!(admitted(&ran)["signer"], "owner");
    let legacy = warned(&ran, "orders::signers::legacy");
    assert_eq!(legacy.len(), 1, "{}", text(&ran.stderr));
    assert!(
        legacy[0].contains(&allowed.display().to_string()),
        "{}",
        legacy[0]
    );
    assert!(legacy[0].contains("orders signers"), "{}", legacy[0]);
    let pinned = ["--json", "call", "orders.pinned"];
    let ran = run_with(
        &personal,
        (stranger.allowed(), allowed.clone()),
        None,
        &pinned,
    );
    assert_eq!(admitted(&ran)["orders"].as_array().unwrap().len(), 2);
    assert_eq!(
        warned(&ran, "orders::signers::legacy").len(),
        1,
        "{}",
        text(&ran.stderr)
    );
    let ran = run_with(&personal, (allowed.clone(), allowed), None, &VERIFY);
    admitted(&ran);
    assert_eq!(
        warned(&ran, "orders::signers::legacy"),
        Vec::<String>::new()
    );
}

/// `orders signers` prints Board's file with the attention file's key
/// merged in for orders and writes neither file; Board's file holding the
/// printed lines verifies the order with no legacy warning.
#[test]
fn signers_prints_boards_file_with_the_attention_key_merged_in_and_those_lines_verify_the_order() {
    use board_model::APPROVAL_HEADER;

    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    signed(&personal, "push");
    let attention = personal.signer.allowed();
    let public = std::fs::read_to_string(personal.signer.directory().join("key.pub")).unwrap();
    let board_line = format!("owner namespaces=\"{APPROVAL_HEADER}\" {}", public.trim());
    personal
        .signer
        .write("board_signers", Item::File(&format!("{board_line}\n")));
    let board = personal.signer.directory().join("board_signers");
    let before = (
        std::fs::read(&board).unwrap(),
        std::fs::read(&attention).unwrap(),
    );
    let ran = run_with(
        &personal,
        (board.clone(), attention.clone()),
        None,
        &["--json", "call", "orders.signers"],
    );
    let output = admitted(&ran);
    assert_eq!(
        output["text"],
        format!(
            "owner namespaces=\"{APPROVAL_HEADER},{ORDER_TYPE}\" {}\n",
            public.trim()
        )
    );
    assert_eq!(output["file"], board.display().to_string());
    assert_eq!(output["changes"], json!([{"line": 1, "change": "gained"}]));
    assert_eq!(output["duplicates"], json!([]));
    assert_eq!(output["left_out"], json!([]));
    assert_eq!(
        before,
        (
            std::fs::read(&board).unwrap(),
            std::fs::read(&attention).unwrap()
        )
    );
    let absent = personal.signer.directory().join("absent");
    let legacy = run_with(&personal, (board.clone(), absent.clone()), None, &VERIFY);
    assert_eq!(refused(&legacy), "orders::order::bad_signature");
    personal.signer.write(
        "board_signers",
        Item::File(output["text"].as_str().unwrap()),
    );
    let ran = run_with(&personal, (board, absent), None, &VERIFY);
    admitted(&ran);
    assert_eq!(
        warned(&ran, "orders::signers::legacy"),
        Vec::<String>::new()
    );
}

/// The `board::plugin::former_name` warnings `output` printed.
fn former(output: &Output) -> Vec<String> {
    (text(&output.stderr).lines())
        .filter_map(|line| line.strip_prefix("warning: board::plugin::former_name: "))
        .map(str::to_owned)
        .collect()
}

/// `attention.*` calls and `attention` in a board's plugin list reach the
/// orders plugin for this release, each with a warning naming `orders`;
/// `orders` itself warns of nothing.
#[test]
fn an_attention_call_or_plugin_list_entry_answers_as_orders_with_a_warning_naming_orders() {
    let personal = personal();
    draft(&personal, "push", &json!({"pinned": true}), CONTEXT);
    signed(&personal, "push");
    let called =
        "attention is the former name of the orders plugin; call it as `board call orders.pinned`";
    let ran = personal.run(&["--json", "call", "attention.pinned"]);
    assert_eq!(admitted(&ran)["orders"][0]["path"], "/orders/push");
    assert_eq!(former(&ran), [called]);
    let ran = personal.run(&["--json", "call", "orders.pinned"]);
    assert_eq!(admitted(&ran)["orders"][0]["path"], "/orders/push");
    assert_eq!(former(&ran), Vec::<String>::new());

    let old = Personal::enabling(&["attention"]);
    draft(&old, "push", &json!({"pinned": true}), CONTEXT);
    signed(&old, "push");
    let enabled = "the board's .board/board.json enables attention, the former name of the orders plugin; `board plugin enable orders` adds its name";
    let ran = old.run(&["--json", "call", "orders.pinned"]);
    assert_eq!(admitted(&ran)["orders"][0]["path"], "/orders/push");
    assert_eq!(former(&ran), [enabled]);
}

/// While the link to the Python plugin, named attention, is still
/// installed, `attention.verify` names both it and the orders plugin, so
/// the owner knows what to remove, and `orders.verify` answers. Without the
/// link `attention.verify` reaches the orders plugin.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "an installed plugin is a link in Board's data directory, which no board tree owns"
)]
fn an_installed_link_to_the_old_plugin_makes_attention_ambiguous_and_without_it_the_call_runs() {
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
    let files = || (personal.signer.allowed(), personal.signer.allowed());
    let attention = [
        "--json",
        "call",
        "attention.verify",
        "--path",
        "/orders/push",
    ];

    let ran = run_with(&personal, files(), Some(plugins.clone()), &attention);
    assert_eq!(refused(&ran), "board::plugin::ambiguous");
    assert_eq!(
        json(&ran)["fields"]["plugins"],
        json!(["orders", "attention"])
    );
    admitted(&run_with(
        &personal,
        files(),
        Some(plugins.clone()),
        &VERIFY,
    ));

    std::fs::remove_file(&link).unwrap();
    admitted(&run_with(&personal, files(), Some(plugins), &attention));
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
        (&json!("/orders/draft"), &json!("orders::order::unsigned"))
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
        &["orders"],
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
    assert_eq!(
        (&pinned(&personal)["orders"], &pinned(&personal)["left_out"]),
        (&json!([]), &json!([]))
    );
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
                (&json!("/orders/b"), &json!("orders::pinned::over_budget"))
            );
        }
    }
}

/// A signers file with no `owner` line, so signing is off.
fn no_owner(personal: &Personal) -> PathBuf {
    let file = personal.signer.directory().join("no_owner");
    personal
        .signer
        .write("no_owner", Item::File("ana ssh-ed25519 AAAA\n"));
    file
}

/// Sets `signing` in the personal board's config to `setting`.
fn set_signing(personal: &Personal, setting: &Value) {
    let mut config = personal.record("/.board/board").unwrap();
    config["signing"] = setting.clone();
    personal.write(".board/board.json", &config.to_string());
}

/// Writes the owner's answer choosing `approve` in a question that names
/// `digest`, asked before it, and returns the answer's path. It was
/// answered a minute ago, so it is recent enough to approve an edit of
/// signing.
fn approving_answer(personal: &Personal, digest: &str) -> &'static str {
    let ago = |seconds: u64| {
        board_model::utc(std::time::SystemTime::now() - std::time::Duration::from_secs(seconds))
    };
    let question = json!({
        "$type": "plans/question/v1",
        "question": format!("Approve the order /orders/push at {digest}?"),
        "asked": ago(120),
        "by": {"session": "local_1"},
        "status": "open",
        "options": [{"id": "approve", "text": "Approve"}, {"id": "refuse", "text": "Refuse"}],
    });
    let answer = json!({
        "$type": "plans/answer/v1",
        "question": "/projects/p/questions/q",
        "choice": "approve",
        "words": "yes",
        "answered": ago(60),
        "by": {"person": "ana"},
    });
    personal.write("projects/p/questions/q.json", &question.to_string());
    personal.write("projects/p/answers/q.json", &answer.to_string());
    "/projects/p/answers/q"
}

/// The digest an unsigned order's refusal names, for its question.
fn digest_of(personal: &Personal) -> String {
    let ran = call(personal, "verify", "/orders/push");
    assert_eq!(refused(&ran), "orders::order::unsigned");
    json(&ran)["fields"]["digest"].as_str().unwrap().to_owned()
}

fn signing_files(personal: &Personal, board: PathBuf) -> (PathBuf, PathBuf) {
    let attention = personal.signer.directory().join("no_attention");
    (board, attention)
}

#[test]
fn an_order_approved_in_chat_verifies_only_where_signing_takes_an_answer_and_says_not_signed() {
    let personal = personal();
    draft(&personal, "push", &json!({"pinned": true}), CONTEXT);
    let digest = digest_of(&personal);
    let answer = approving_answer(&personal, &digest);
    let mut stored = personal.record("/orders/push").unwrap();
    stored["approved"] = json!(answer);
    personal.write("orders/push.json", &stored.to_string());
    let off = signing_files(&personal, no_owner(&personal));
    let ran = run_with(&personal, off.clone(), None, &VERIFY);
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let verified = json(&ran)["data"]["output"].clone();
    assert_eq!(verified["approval"], "approved in chat, not signed");
    let ran = run_with(
        &personal,
        off.clone(),
        None,
        &["--json", "call", "orders.pinned"],
    );
    let listed = json(&ran)["data"]["output"].clone();
    assert_eq!(
        listed["signing"],
        "Signing is off: every act takes an answer in chat."
    );
    assert_eq!(
        listed["orders"][0]["approval"],
        "approved in chat, not signed"
    );
    assert_eq!(
        refused(&call(&personal, "verify", "/orders/push")),
        "orders::order::unsigned"
    );
    set_signing(&personal, &json!({"require": ["effect"]}));
    admitted(&call(&personal, "verify", "/orders/push"));
    personal.write("orders/push.md", "an edited context.\n");
    let ran = run_with(&personal, off, None, &VERIFY);
    assert_eq!(refused(&ran), "orders::order::not_approved");
}

#[test]
fn an_order_approved_by_an_answer_with_no_person_does_not_verify_with_signing_off() {
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    let digest = digest_of(&personal);
    let answer = approving_answer(&personal, &digest);
    let mut unnamed = personal.record(answer).unwrap();
    unnamed["by"] = json!({"session": "local_1"});
    personal.write("projects/p/answers/q.json", &unnamed.to_string());
    let mut stored = personal.record("/orders/push").unwrap();
    stored["approved"] = json!(answer);
    personal.write("orders/push.json", &stored.to_string());
    let off = signing_files(&personal, no_owner(&personal));
    assert_eq!(
        refused(&run_with(&personal, off, None, &VERIFY)),
        "orders::order::not_approved"
    );
}

#[test]
fn sign_with_an_approving_answer_records_it_where_signing_takes_an_answer_and_is_refused_where_orders_need_touch_id()
 {
    let personal = personal();
    draft(&personal, "push", &json!({}), CONTEXT);
    let digest = digest_of(&personal);
    let answer = approving_answer(&personal, &digest);
    let sign = [
        "--json",
        "call",
        "orders.sign",
        "--path",
        "/orders/push",
        "--approved",
        answer,
    ];
    assert_eq!(refused(&personal.run(&sign)), "orders::order::unsigned");
    assert_eq!(
        personal.record("/orders/push").unwrap().get("approved"),
        None
    );
    let off = signing_files(&personal, no_owner(&personal));
    let ran = run_with(&personal, off.clone(), None, &sign);
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let stored = personal.record("/orders/push").unwrap();
    assert_eq!(
        (stored.get("signature"), &stored["approved"]),
        (None, &json!(answer))
    );
    assert_eq!(
        json(&run_with(&personal, off, None, &VERIFY))["data"]["output"]["approval"],
        "approved in chat, not signed"
    );
}

#[test]
fn pinned_and_signers_start_with_the_signing_state_for_off_on_and_a_shortened_list() {
    let personal = personal();
    let line = |files: Option<(PathBuf, PathBuf)>, capability: &str| {
        let arguments = ["--json", "call", capability];
        let ran = match files {
            Some(files) => run_with(&personal, files, None, &arguments),
            None => personal.run(&arguments),
        };
        assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
        json(&ran)["data"]["output"]["signing"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let off = signing_files(&personal, no_owner(&personal));
    assert_eq!(
        line(Some(off.clone()), "orders.pinned"),
        "Signing is off: every act takes an answer in chat."
    );
    assert_eq!(
        line(Some(off.clone()), "orders.signers"),
        format!("Signing is off: {} has no owner line.", off.0.display())
    );
    let on = "Signing is on: orders, effects and owner's writes need Touch ID.";
    assert_eq!(line(None, "orders.pinned"), on);
    assert_eq!(line(None, "orders.signers"), on);
    set_signing(&personal, &json!({"require": ["order", "plan"]}));
    assert_eq!(
        line(None, "orders.pinned"),
        "Signing is on: orders and plans need Touch ID."
    );
    assert_eq!(
        line(None, "orders.signers"),
        "Signing is on: orders and plans need Touch ID."
    );
}

#[test]
fn pinned_and_signers_as_text_start_with_the_signing_line_and_list_orders_and_lines_as_rows() {
    let personal = personal();
    draft(&personal, "push", &json!({"pinned": true}), CONTEXT);
    signed(&personal, "push");
    let ran = personal.run(&["orders", "pinned"]);
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
    assert_eq!(
        text(&ran.stdout),
        "Signing is on: orders, effects and owner's writes need Touch ID.\n\
         \n\
         ok /orders/push  signed by owner  expires 2999-12-31\n"
    );
    let ran = personal.run(&["orders", "signers"]);
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
    let printed = text(&ran.stdout);
    let file = admitted(&personal.run(&["--json", "call", "orders.signers"]))["file"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut lines = printed.lines();
    assert_eq!(
        lines.next(),
        Some("Signing is on: orders, effects and owner's writes need Touch ID.")
    );
    assert_eq!(
        lines.next(),
        Some(format!("# {file}").as_str()),
        "{printed}"
    );
    assert!(
        lines.next().is_some_and(|line| line.starts_with("owner ")),
        "{printed}"
    );
}

#[test]
fn with_signing_off_a_put_that_edits_require_needs_an_approving_answer_and_one_of_configured_is_refused()
 {
    let personal = personal();
    let off = signing_files(&personal, no_owner(&personal));
    let mut config = personal.record("/.board/board").unwrap();
    config["signing"] = json!({"require": ["plan"]});
    let put = |config: &Value, approved: Option<&str>| {
        let mut arguments = vec!["--json", "put", "/.board/board"];
        arguments.extend(
            approved
                .map(|answer| ["--approved", answer])
                .into_iter()
                .flatten(),
        );
        run_reading(
            &personal,
            off.clone(),
            None,
            &arguments,
            &config.to_string(),
        )
    };
    let bare = put(&config, None);
    assert_eq!(refused(&bare), "board::signing::not_approved");
    let digest = board_model::setting_digest(None, Some(&config["signing"]));
    assert!(
        json(&bare)["message"].as_str().unwrap().contains(&digest),
        "{}",
        text(&bare.stdout)
    );
    let answer = approving_answer(&personal, &digest);
    let ran = put(&config, Some(answer));
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
    assert_eq!(
        personal.record("/.board/board").unwrap()["signing"],
        config["signing"]
    );
    let from = config["signing"].clone();
    config["signing"] = json!({"configured": false});
    let answer = approving_answer(
        &personal,
        &board_model::setting_digest(Some(&from), Some(&config["signing"])),
    );
    assert_eq!(
        refused(&put(&config, Some(answer))),
        "board::settings::unsigned"
    );
}

/// A pending effect's id, for a seen record the tests write by hand.
const SEEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// `board --json put <path> [--approved <answer>]` in the personal board
/// with `files` as the allowed signers files and `body` on stdin.
fn put_with(
    personal: &Personal,
    files: (PathBuf, PathBuf),
    path: &str,
    body: &Value,
    approved: Option<&str>,
) -> Output {
    let mut arguments = vec!["--json", "put", path];
    arguments.extend(
        approved
            .map(|answer| ["--approved", answer])
            .into_iter()
            .flatten(),
    );
    run_reading(personal, files, None, &arguments, &body.to_string())
}

/// With a signed seen record in the personal board and the allowed signers
/// file gone, `board put` of the config with a new `require` and an
/// approving answer, a put under `effects/` and a removal there are all
/// refused as `file_missing`; with the file there, the same edit needs the
/// owner's signature.
#[test]
fn with_a_signed_seen_record_and_the_signers_file_gone_an_edit_of_require_or_a_write_under_effects_is_file_missing_and_with_the_file_it_needs_a_signature()
 {
    let personal = personal();
    let seen = json!({"$type": board_model::SEEN_TYPE, "signature": "signed"});
    personal.write(&format!("effects/seen/{SEEN}.json"), &seen.to_string());
    let gone = signing_files(&personal, personal.signer.directory().join("gone"));
    let mut config = personal.record("/.board/board").unwrap();
    config["signing"] = json!({"require": []});
    let answer = approving_answer(
        &personal,
        &board_model::setting_digest(None, Some(&config["signing"])),
    );
    let ran = put_with(
        &personal,
        gone.clone(),
        "/.board/board",
        &config,
        Some(answer),
    );
    assert_eq!(refused(&ran), "board::signing::file_missing");
    assert_eq!(
        personal.record("/.board/board").unwrap().get("signing"),
        None
    );
    let order = json!({"$type": ORDER_TYPE, "order": "Do not push to main.", "context": "/orders/push.md", "expires": "2999-12-31"});
    let ran = put_with(&personal, gone.clone(), "/effects/seen/s", &order, None);
    assert_eq!(refused(&ran), "board::signing::file_missing");
    let removed = format!("/effects/seen/{SEEN}");
    let ran = run_with(&personal, gone.clone(), None, &["--json", "rm", &removed]);
    assert_eq!(refused(&ran), "board::signing::file_missing");
    assert!(personal.record(&removed).is_some());
    let ran = put_with(&personal, gone, "/orders/other", &order, None);
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
    signer(&personal, "#!/bin/sh\nprintf 'Not now.\\n'\nexit 3\n");
    let present = signing_files(&personal, personal.signer.allowed());
    let ran = put_with(&personal, present, "/.board/board", &config, Some(answer));
    assert_eq!(refused(&ran), "board::settings::unsigned");
    assert_eq!(
        personal.record("/.board/board").unwrap().get("signing"),
        None
    );
}

/// A known boards file that names a directory that is not a board refuses
/// only a write that reads signing's state, and leaves other puts alone.
#[test]
fn a_personal_board_that_does_not_open_refuses_a_write_under_effects_and_not_a_put_elsewhere() {
    let personal = personal();
    let elsewhere = tempfile::tempdir().unwrap();
    let known = json!({"personal": elsewhere.path()}).to_string();
    personal.xdg.write("board/config.json", Item::File(&known));
    let files = signing_files(&personal, no_owner(&personal));
    let order = json!({"$type": ORDER_TYPE, "order": "Do not push to main.", "context": "/orders/push.md", "expires": "2999-12-31"});
    let ran = put_with(&personal, files.clone(), "/orders/push", &order, None);
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
    let ran = put_with(&personal, files, "/effects/seen/s", &order, None);
    assert_eq!(refused(&ran), "board::config::not_found");
    assert!(personal.record("/effects/seen/s").is_none());
}

/// A known boards file that names a second, empty board as the personal
/// one does not turn signing off for the board being written: with a
/// signed seen record there and the allowed signers file gone, an edit of
/// `require` and a put under `effects/` are refused as `file_missing`, and
/// a put elsewhere goes ahead.
#[test]
fn a_known_boards_file_naming_an_empty_board_keeps_the_written_boards_signed_record_refusing_a_missing_signers_file()
 {
    let (personal, decoy) = (personal(), personal());
    let known = json!({"personal": decoy.board.workdir()}).to_string();
    personal.xdg.write("board/config.json", Item::File(&known));
    let seen = json!({"$type": board_model::SEEN_TYPE, "signature": "signed"});
    personal.write(&format!("effects/seen/{SEEN}.json"), &seen.to_string());
    let gone = signing_files(&personal, personal.signer.directory().join("gone"));
    let mut config = personal.record("/.board/board").unwrap();
    config["signing"] = json!({"require": []});
    let answer = approving_answer(
        &personal,
        &board_model::setting_digest(None, Some(&config["signing"])),
    );
    let ran = put_with(
        &personal,
        gone.clone(),
        "/.board/board",
        &config,
        Some(answer),
    );
    assert_eq!(refused(&ran), "board::signing::file_missing");
    let order = json!({"$type": ORDER_TYPE, "order": "Do not push to main.", "context": "/orders/push.md", "expires": "2999-12-31"});
    let ran = put_with(&personal, gone.clone(), "/effects/seen/s", &order, None);
    assert_eq!(refused(&ran), "board::signing::file_missing");
    let ran = put_with(&personal, gone, "/orders/other", &order, None);
    assert_eq!(ran.code, 0, "{}{}", text(&ran.stdout), text(&ran.stderr));
}
