//! The skills a run's phases rely on that the agent's home lacks.
//!
//! A phase names in `skills` the skills its instruction relies on. A skill
//! is installed when `<HOME>/.claude/skills/<name>/SKILL.md` exists, which
//! is where Claude Code reads skills and where `board skills install`
//! writes them unless `--dir` names another directory.

use std::path::PathBuf;

use board_ops::Host;
use schemars::JsonSchema;
use serde::Serialize;

use crate::flow::Flow;

/// The skills directory under the home, as `board skills install` writes
/// it by default.
const SKILLS_DIRECTORY: &str = ".claude/skills";

/// The skills the phases of a run name that are not installed, in the
/// order the phases name them, with the directory they were looked for in
/// and what installs them.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct MissingSkills {
    names: Vec<String>,
    directory: String,
    help: String,
}

/// The skills that `phases` of `flow` name and the home `host` passes on
/// lacks; none when every one is installed or HOME is not set.
pub(super) fn missing(host: &dyn Host, flow: &Flow, phases: &[&str]) -> Option<MissingSkills> {
    let home = (host.settings().environment.0.iter())
        .find(|(name, _)| name == "HOME")
        .map(|(_, value)| PathBuf::from(value))?;
    let directory = home.join(SKILLS_DIRECTORY);
    let mut names: Vec<String> = Vec::new();
    for phase in (flow.phases.iter()).filter(|phase| phases.contains(&phase.name.as_str())) {
        for skill in &phase.skills {
            let installed = directory.join(skill).join("SKILL.md").is_file();
            if !installed && !names.contains(skill) {
                names.push(skill.clone());
            }
        }
    }
    if names.is_empty() {
        return None;
    }
    let shown = directory.display().to_string();
    Some(MissingSkills {
        help: format!(
            "the run's phases rely on these skills, which are not in {shown}; install Board's \
             skills with `board skills install`, or install the others there"
        ),
        names,
        directory: shown,
    })
}
