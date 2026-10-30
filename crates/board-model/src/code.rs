//! The stable codes that name every refusal and every degraded entry.

use std::fmt;

mod hint;

/// Declares `Code` and `ALL` in one order, with each code's spelling.
macro_rules! codes {
    ($($variant:ident => $spelling:literal,)*) => {
        /// A code, `board::<area>::<reason>`, which changes as STANDARDS.md 3 and 4
        /// say. docs/architecture/errors.md lists the codes.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Code {
            $($variant,)*
        }

        impl Code {
            /// Every code, so a spelling can be read back.
            pub const ALL: [Code; [$($spelling),*].len()] = [$(Self::$variant),*];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $spelling,)*
                }
            }
        }
    };
}

codes! {
    JsonParse => "board::json::parse",
    JsonMergeConflict => "board::json::merge_conflict",
    JsonDuplicateKey => "board::json::duplicate_key",
    JsonTooLarge => "board::json::too_large",
    JsonNotUtf8 => "board::json::not_utf8",
    JsonLossyNumber => "board::json::lossy_number",
    FrontmatterParse => "board::frontmatter::parse",
    FrontmatterUnsupported => "board::frontmatter::unsupported",
    RecordBadType => "board::record::bad_type",
    RecordUnknownType => "board::record::unknown_type",
    RecordInvalid => "board::record::invalid",
    RecordNotARecord => "board::record::not_a_record",
    ConfigNotFound => "board::config::not_found",
    ConfigInvalid => "board::config::invalid",
    ConfigAmbiguous => "board::config::ambiguous",
    SchemaInvalid => "board::schema::invalid",
    SchemaDiffers => "board::schema::differs",
    RefMalformed => "board::ref::malformed",
    RefWrongType => "board::ref::wrong_type",
    PathMalformed => "board::path::malformed",
    PathNotFound => "board::path::not_found",
    PathShadowed => "board::path::shadowed",
    PathCaseCollision => "board::path::case_collision",
    RefDangling => "board::ref::dangling",
    MountInvalid => "board::mount::invalid",
    MountRepoMissing => "board::mount::repo_missing",
    MountCommitMissing => "board::mount::commit_missing",
    MountRemoteUnsupported => "board::mount::remote_unsupported",
    MountFollowMissing => "board::mount::follow_missing",
    MountNotAncestor => "board::mount::not_ancestor",
    MountStale => "board::mount::stale",
    ResolveCycle => "board::resolve::cycle",
    ResolveTooManyHops => "board::resolve::too_many_hops",
    RepoUnreadable => "board::repo::unreadable",
    RepoSubmodule => "board::repo::submodule",
    SymlinkOutside => "board::symlink::outside",
    SymlinkLoop => "board::symlink::loop",
    SymlinkDangling => "board::symlink::dangling",
    IndexFailed => "board::index::failed",
    QueryInvalid => "board::query::invalid",
    QueryTimeout => "board::query::timeout",
    OutputOverBudget => "board::output::over_budget",
    PutStale => "board::put::stale",
    PutLocked => "board::put::locked",
    PutThroughMount => "board::put::through_mount",
    PutSymlink => "board::put::symlink",
    PutInvalid => "board::put::invalid",
    PutNotAFile => "board::put::not_a_file",
    PutNul => "board::put::nul",
    PutNoIdentity => "board::put::no_identity",
    PutHeadMoved => "board::put::head_moved",
    PutFailed => "board::put::failed",
    RmDirectory => "board::rm::directory",
    SyncNoRemote => "board::sync::no_remote",
    SyncFailed => "board::sync::failed",
    SyncConflict => "board::sync::conflict",
    SyncEdited => "board::sync::edited",
    SyncKept => "board::sync::kept",
    SyncGaveUp => "board::sync::gave_up",
    SyncTimeout => "board::sync::timeout",
    RefsTargetChanged => "board::refs::target_changed",
    AttachmentMissing => "board::attachment::missing",
    AttachmentMismatch => "board::attachment::mismatch",
    PluginNotInstalled => "board::plugin::not_installed",
    PluginBadManifest => "board::plugin::bad_manifest",
    PluginNotEnabled => "board::plugin::not_enabled",
    PluginSchemaMissing => "board::plugin::schema_missing",
    PluginSchemaDiffers => "board::plugin::schema_differs",
    PluginAmbiguous => "board::plugin::ambiguous",
    PluginBadInput => "board::plugin::bad_input",
    PluginBadOutput => "board::plugin::bad_output",
    PluginFailed => "board::plugin::failed",
    PluginNotStarted => "board::plugin::not_started",
    PluginTimeout => "board::plugin::timeout",
    PluginTooLarge => "board::plugin::too_large",
    PluginTooDeep => "board::plugin::too_deep",
    SettingsAsked => "board::settings::asked",
    SettingsRefused => "board::settings::refused",
    SettingsFromPlugin => "board::settings::from_plugin",
    SettingsNotFound => "board::settings::not_found",
    SettingsNoPersonal => "board::settings::no_personal",
    SettingsUnsigned => "board::settings::unsigned",
    UiUnknownKind => "board::ui::unknown_kind",
    UiSlotMismatch => "board::ui::slot_mismatch",
    UiSlotInvalid => "board::ui::slot_invalid",
    UiNotABinding => "board::ui::not_a_binding",
    UiOpenFailed => "board::ui::open_failed",
    ServeHost => "board::serve::host",
    ServeOrigin => "board::serve::origin",
    ServeToken => "board::serve::token",
    ServeCodeUnknown => "board::serve::code_unknown",
    ServeCodeUsed => "board::serve::code_used",
    ServeCodeExpired => "board::serve::code_expired",
    ServeUnknownRoute => "board::serve::unknown_route",
    ServeFailed => "board::serve::failed",
    CliUsage => "board::cli::usage",
}

impl Code {
    /// The code spelled `text`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|code| code.as_str() == text)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{Fields, LogicalPath, MAX_SYMLINKS};

    #[test]
    fn every_code_is_board_area_reason_in_snake_case() {
        for code in Code::ALL {
            let parts: Vec<&str> = code.as_str().split("::").collect();
            assert_eq!(parts.len(), 3, "{code}");
            assert_eq!(parts[0], "board", "{code}");
            for part in &parts[1..] {
                assert!(!part.is_empty(), "{code}");
                assert!(
                    part.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                    "{code}"
                );
            }
        }
    }

    #[test]
    fn no_two_codes_share_a_spelling() {
        let spellings: HashSet<&str> = Code::ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(spellings.len(), Code::ALL.len());
    }

    #[test]
    fn every_code_parses_back_from_its_spelling() {
        for code in Code::ALL {
            assert_eq!(Code::parse(code.as_str()), Some(code));
        }
        assert_eq!(Code::parse("board::json::parse "), None);
    }

    #[test]
    fn a_code_displays_as_its_spelling() {
        assert_eq!(
            Code::ResolveTooManyHops.to_string(),
            "board::resolve::too_many_hops"
        );
    }

    #[test]
    fn a_mount_hint_names_its_own_remedy_with_the_mount_path_and_without_one_the_generic_hint() {
        let mount: LogicalPath = "/vendor".parse().unwrap();
        let named = Fields::at(&mount);
        let cases = [
            (
                Code::MountNotAncestor,
                "to pin the mount to the mounted HEAD",
            ),
            (Code::MountStale, "reapply your change"),
        ];
        for (code, remedy) in cases {
            let hint = code.hint(&named);
            assert!(hint.contains("`board mount update /vendor`"), "{hint}");
            assert!(hint.contains(remedy), "{hint}");
            assert!(code.hint(&Fields::default()).contains("<path>"));
        }
        let other = Code::PutThroughMount.hint(&named);
        assert!(!other.contains("reapply your change"), "{other}");
    }

    #[test]
    fn a_symlink_hint_names_the_symlink_text_and_its_own_remedy_and_in_a_mount_sends_the_fix_there()
    {
        let mount: LogicalPath = "/vendor".parse().unwrap();
        let path: LogicalPath = "/vendor/docs/l".parse().unwrap();
        let named = Fields {
            target: Some("../x.json".to_owned()),
            ..Fields::at(&path)
        };
        let cases = [
            (
                Code::SymlinkOutside,
                "starts at `/` or leaves the repository",
            ),
            (
                Code::SymlinkLoop,
                &format!("more than {MAX_SYMLINKS} symlinks"),
            ),
            (
                Code::SymlinkDangling,
                "no file the board holds is at `../x.json`",
            ),
        ];
        for (code, remedy) in cases {
            let hint = code.hint(&named);
            assert!(
                hint.contains("`../x.json`") && hint.contains(remedy),
                "{hint}"
            );
            let mounted = code.hint_in_mount(&named, &mount);
            assert_eq!(
                mounted,
                format!(
                    "/vendor/docs/l is in the repository of the board mounted at /vendor, which Board cannot write; there, {hint}, then run `board mount update /vendor`"
                )
            );
            let unnamed = Fields::at(&path);
            assert_eq!(code.hint(&unnamed), code.hint(&Fields::default()));
            assert!(!code.hint(&unnamed).contains("../x.json"));
        }
    }

    #[test]
    fn a_record_its_schema_refuses_hints_at_the_pointer_that_failed() {
        let fields = Fields {
            pointer: Some("/due".to_owned()),
            ..Fields::default()
        };
        assert_eq!(
            Code::RecordInvalid.hint(&fields),
            "change the value at `/due` so the record satisfies its schema in `.board/types/`"
        );
    }

    #[test]
    fn a_hint_for_a_file_in_a_mounted_board_sends_the_fix_to_that_boards_repository() {
        let mount: LogicalPath = "/vendor".parse().unwrap();
        let fields = Fields {
            path: Some("/vendor/a".parse().unwrap()),
            schema: Some(".board/types/task.json".to_owned()),
            ..Fields::default()
        };
        assert_eq!(
            Code::SchemaInvalid.hint_in_mount(&fields, &mount),
            "fix the schema at `.board/types/task.json` in the repository of the board \
             mounted at /vendor; `board check /vendor/.board/types` says what failed"
        );
        for code in [
            Code::MountCommitMissing,
            Code::RecordUnknownType,
            Code::RefDangling,
            Code::JsonParse,
        ] {
            let hint = code.hint_in_mount(&fields, &mount);
            assert!(
                hint.contains("in the repository of the board mounted at /vendor"),
                "{code}: {hint}"
            );
        }
        assert_eq!(
            Code::PathNotFound.hint_in_mount(&fields, &mount),
            Code::PathNotFound.hint(&fields),
            "a code with no mounted hint"
        );
    }
}
