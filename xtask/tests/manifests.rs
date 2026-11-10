//! Every workspace member carries the workspace's licence and repository,
//! so each crate names them in its metadata.

use std::path::Path;

/// The fields a member's `[package]` must inherit from the workspace.
const INHERITED: &[&str] = &["license", "repository"];

/// The inherited fields `manifest`'s `[package]` does not take from the
/// workspace with `<field>.workspace = true`, or `None` when `manifest`
/// is not TOML with a `[package]` table.
fn not_inherited(manifest: &str) -> Option<Vec<&'static str>> {
    let manifest: toml::Table = toml::from_str(manifest).ok()?;
    let package = manifest.get("package")?.as_table()?;
    let missing = INHERITED
        .iter()
        .copied()
        .filter(|field| {
            let workspace = (package.get(*field).and_then(toml::Value::as_table))
                .and_then(|field| field.get("workspace"));
            workspace.and_then(toml::Value::as_bool) != Some(true)
        })
        .collect();
    Some(missing)
}

#[test]
fn a_manifest_that_does_not_inherit_the_licence_and_repository_is_named_and_one_that_does_is_not() {
    let inheriting =
        "[package]\nname = \"a\"\nlicense.workspace = true\nrepository.workspace = true\n";
    assert_eq!(not_inherited(inheriting), Some(Vec::new()));
    let own = "[package]\nname = \"a\"\nlicense = \"MIT\"\nrepository.workspace = true\n";
    assert_eq!(not_inherited(own), Some(vec!["license"]));
    let neither = "[package]\nname = \"a\"\n";
    assert_eq!(not_inherited(neither), Some(vec!["license", "repository"]));
}

#[test]
fn every_workspace_member_inherits_the_licence_and_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let workspace: toml::Table =
        toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
    let package = workspace["workspace"]["package"].as_table().unwrap();
    assert_eq!(package["license"].as_str(), Some("MIT OR Apache-2.0"));
    assert_eq!(
        package["repository"].as_str(),
        Some("https://github.com/crumbkit/board")
    );
    let members = workspace["workspace"]["members"].as_array().unwrap();
    let missing: Vec<(String, Vec<&str>)> = members
        .iter()
        .map(|member| member.as_str().unwrap().to_owned())
        .filter_map(|member| {
            let manifest = std::fs::read_to_string(root.join(&member).join("Cargo.toml")).unwrap();
            let missing = not_inherited(&manifest).unwrap();
            (!missing.is_empty()).then_some((member, missing))
        })
        .collect();
    assert_eq!(missing, Vec::new());
}
