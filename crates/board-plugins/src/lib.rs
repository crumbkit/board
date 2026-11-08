//! The plugins built into the `board` binary, each behind a Cargo feature
//! of its name (docs/plugins/built-in.md).

use board_ops::BuiltIn;

/// Every plugin this build of Board has built in.
pub const BUILT_IN: BuiltIn = BuiltIn(&[
    #[cfg(feature = "artifacts")]
    &board_plugin_artifacts::Artifacts,
    #[cfg(feature = "attention")]
    &board_plugin_attention::Attention,
    #[cfg(feature = "boundary")]
    &board_plugin_boundary::Boundary,
    #[cfg(feature = "changes")]
    &board_plugin_changes::Changes,
    #[cfg(feature = "controls")]
    &board_plugin_controls::Controls,
    #[cfg(feature = "evals")]
    &board_plugin_evals::Evals,
    #[cfg(feature = "github")]
    &board_plugin_github::Github,
    #[cfg(feature = "jira")]
    &board_plugin_jira::Jira,
    #[cfg(feature = "notes")]
    &board_plugin_notes::Notes,
    #[cfg(feature = "params")]
    &board_plugin_params::Params,
    #[cfg(feature = "plans")]
    &board_plugin_plans::Plans,
    #[cfg(feature = "prompts")]
    &board_plugin_prompts::Prompts,
    #[cfg(feature = "requests")]
    &board_plugin_requests::Requests,
    #[cfg(feature = "review")]
    &board_plugin_review::Review,
    #[cfg(feature = "runs")]
    &board_plugin_runs::Runs,
    #[cfg(feature = "share")]
    &board_plugin_share::Share,
    #[cfg(feature = "tasks")]
    &board_plugin_tasks::Tasks,
    #[cfg(feature = "topics")]
    &board_plugin_topics::Topics,
]);
