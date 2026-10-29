use super::*;
use crate::config::Config;
use crate::report::{Report, render};
use crate::tests::Fixture;

/// This repository's own `[signing]` section, so the tests use its
/// literals and owners without spelling a literal here.
fn repository() -> SigningConfig {
    let root = Root::open(std::path::Path::new(crate::ROOT)).unwrap();
    Config::load(&root).unwrap().signing
}

fn rendered(check: Check) -> String {
    render(&Report {
        checks: vec![check],
        controls: Vec::new(),
    })
}

#[test]
fn a_namespace_literal_outside_the_owners_is_refused_with_the_file_line_and_literal() {
    let config = repository();
    let literal = &config.literals[0];
    let fixture = Fixture::new();
    fixture.write(
        "crates/board-ops/src/effects.rs",
        &format!("fn a() {{}}\nconst A: &str = \"{literal}\";\n"),
    );
    let check = check(&fixture.root(), &config).unwrap();
    assert!(!check.passed());
    assert_eq!(
        rendered(check),
        format!(
            "FAIL  signing      1 = 0     literals    crates/board-ops/src/effects.rs:2 names \"{literal}\"\n      \
             xtask::signing::outside_owner: only the files signing.owners in lint.toml lists may name a \
             signing literal; use board_model's headers or the vectors, or move the code into an owner \
             (docs/architecture/writes/signing.md)\n\
             pass  controls   controls/budgets/ agrees with lint.toml\n"
        )
    );
}

#[test]
fn the_same_literal_in_an_owner_is_admitted() {
    let config = repository();
    let literal = &config.literals[0];
    let fixture = Fixture::new();
    fixture
        .write(
            "crates/board-model/src/effect.rs",
            &format!("fn a() {{}}\nconst A: &str = \"{literal}\";\n"),
        )
        .write(
            "crates/board-model/src/effect/tests.rs",
            &format!("const A: &str = \"{literal}\";\n"),
        );
    let check = check(&fixture.root(), &config).unwrap();
    assert!(check.passed());
    assert_eq!(
        rendered(check),
        "pass  signing      0 = 0     literals    \n\
         pass  controls   controls/budgets/ agrees with lint.toml\n"
    );
}

#[test]
fn a_file_whose_name_only_starts_like_an_owner_is_not_an_owner() {
    let config = SigningConfig {
        literals: vec!["ns/v1".to_owned()],
        owners: vec!["src/effect".to_owned(), "src/sign.rs".to_owned()],
    };
    assert!(config.owns("src/effect/tests.rs"));
    assert!(config.owns("src/sign.rs"));
    assert!(!config.owns("src/effects.rs"));
    assert!(!config.owns("src/sign.rs.bak"));
}

#[test]
fn a_literal_in_a_python_shell_typescript_or_bin_script_is_refused_and_in_docs_controls_or_data_is_admitted()
 {
    let config = SigningConfig {
        literals: vec!["ns/a".to_owned()],
        owners: Vec::new(),
    };
    let refused = Fixture::new();
    for path in [
        "plugins/p/src/p/a.py",
        "bin/setup.sh",
        "ui/src/a.ts",
        "plugins/p/claude-code/a.tsx",
        "bin/approve",
        "plugins/p/bin/run",
    ] {
        refused.write(path, "\"ns/a\"\n");
    }
    let check = check(&refused.root(), &config).unwrap();
    let labels: Vec<&str> = check.parts.iter().map(|part| part.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "bin/approve:1 names \"ns/a\"",
            "bin/setup.sh:1 names \"ns/a\"",
            "plugins/p/bin/run:1 names \"ns/a\"",
            "plugins/p/claude-code/a.tsx:1 names \"ns/a\"",
            "plugins/p/src/p/a.py:1 names \"ns/a\"",
            "ui/src/a.ts:1 names \"ns/a\"",
        ]
    );
    let admitted = Fixture::new();
    for path in [
        "docs/a.md",
        "controls/rules/a.json",
        "vectors/signing/v1/approval.statement",
        "crates/c/README",
        "ui/node_modules/m/a.ts",
    ] {
        admitted.write(path, "\"ns/a\"\n");
    }
    assert!(check_passes(&admitted, &config));
}

fn check_passes(fixture: &Fixture, config: &SigningConfig) -> bool {
    check(&fixture.root(), config).unwrap().passed()
}

#[test]
fn every_literal_on_every_line_is_reported() {
    let config = SigningConfig {
        literals: vec!["ns/a".to_owned(), "ns/b".to_owned()],
        owners: Vec::new(),
    };
    let fixture = Fixture::new();
    fixture.write("src/a.rs", "\"ns/a\" \"ns/b\"\n\n\"ns/a\"\n");
    let check = check(&fixture.root(), &config).unwrap();
    assert_eq!(check.used, 3);
}
