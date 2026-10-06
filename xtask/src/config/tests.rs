use super::*;
use crate::tests::CONFIG;

#[test]
fn a_complete_config_parses() {
    let config = Config::parse(CONFIG).unwrap();
    assert_eq!(config.context.max_words, 10);
    assert_eq!(config.docs.glob, "docs/*.md");
    assert_eq!(config.plan.file, "PLAN.md");
}

#[test]
fn an_unknown_key_is_refused() {
    let text = CONFIG.replace("[expect]", "[expect]\nslack = 1");
    let error = Config::parse(&text).unwrap_err();
    assert_eq!(error.code(), "xtask::config::invalid");
}

#[test]
fn an_unknown_type_tag_is_refused() {
    let text = CONFIG.replace("board/lint-config/v1", "board/lint-config/v2");
    assert!(Config::parse(&text).is_err());
}

#[test]
fn a_missing_budget_is_refused() {
    let text = CONFIG.replace("[source]\nmax_lines = 3\n", "");
    assert!(Config::parse(&text).is_err());
}
