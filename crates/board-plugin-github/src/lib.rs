//! The github plugin built into Board (plugins/github/README.md). Each
//! capability is listed once in `CAPABILITIES`; every call to GitHub goes
//! through `gh`, every git command through `git`, and every call to the
//! boundary through `crossing`.

mod crossing;
mod gh;
mod git;
mod hook;
mod objects;
mod parts;
pub mod records;
mod reference;
mod remote;
mod send;

use board_model::{CapabilityName, RecordType};
use board_ops::{Host, Plugin, PluginError};
use board_plugin::Manifest;
use board_plugin::shape::{Record, schema_of};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use records::{IssueRecord, PullRecord};

#[derive(Clone, Copy, Debug)]
pub struct Github;

impl Plugin for Github {
    fn manifest(&self) -> Manifest {
        Manifest::built_in(manifest()).expect(
            "the manifest is built from constants and its inputs' schemas; a test parses it",
        )
    }

    fn schemas(&self) -> Vec<(RecordType, Value)> {
        let settings = SchemaSettings::draft2020_12;
        vec![
            (
                PullRecord::TYPE
                    .parse()
                    .expect("PullRecord's TYPE is a record type; a test parses it"),
                schema_of::<PullRecord>(settings()),
            ),
            (
                IssueRecord::TYPE
                    .parse()
                    .expect("IssueRecord's TYPE is a record type; a test parses it"),
                schema_of::<IssueRecord>(settings()),
            ),
        ]
    }

    fn call(
        &self,
        host: &dyn Host,
        capability: &CapabilityName,
        input: Value,
    ) -> Result<Value, PluginError> {
        let found = (CAPABILITIES.iter()).find(|offered| offered.name == capability.as_str());
        let Some(offered) = found else {
            let names: Vec<&str> = CAPABILITIES.iter().map(|offered| offered.name).collect();
            return Err(PluginError {
                code: "github::call::unknown_capability".to_owned(),
                message: format!(
                    "github offers no capability {capability}; it offers {}",
                    names.join(", ")
                ),
                fields: Map::new(),
                hint: None,
            });
        };
        (offered.run)(host, &input)
    }
}

/// What a sending action passes on: gh's variables, those the boundary's
/// judged rules read, and the ssh agent git pushes through.
const SEND_ENV: [&str; 7] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_CONFIG_DIR",
    "BOARD_CONTROLS_AGENT",
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "SSH_AUTH_SOCK",
];
/// What the pre-push hook passes on: a sending action's variables, the URL
/// git pushes to, and the variables that make git run another ssh command,
/// which the URL rule reads to fail closed on an ssh alias.
const PRE_PUSH_ENV: [&str; 10] = [
    SEND_ENV[0],
    SEND_ENV[1],
    SEND_ENV[2],
    SEND_ENV[3],
    SEND_ENV[4],
    SEND_ENV[5],
    SEND_ENV[6],
    hook::PUSH_URL,
    remote::SSH_VARIABLES[0],
    remote::SSH_VARIABLES[1],
];
/// How long a sending action may run, in seconds: the boundary's 1,800 for
/// its check, then the push and gh.
const SEND_TIMEOUT: u64 = 2400;

/// One capability: its name, what it does, the function that runs it, its
/// input and output schemas, whether it only reads, the variables it is
/// given and how long it may run.
struct Capability {
    name: &'static str,
    description: &'static str,
    run: fn(&dyn Host, &Value) -> Result<Value, PluginError>,
    input: fn() -> Value,
    output: fn() -> Value,
    read_only: bool,
    env: &'static [&'static str],
    timeout: Option<u64>,
}

fn input<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12())
}

fn output<T: JsonSchema>() -> Value {
    schema_of::<T>(SchemaSettings::draft2020_12().for_serialize())
}

/// Every capability, in the order the manifest lists them.
const CAPABILITIES: [Capability; 9] = [
    Capability {
        name: "ref",
        description: "Write the record of a pull request or issue, asking gh which it is and its \
            title, or report the record already there without asking.",
        run: reference::reference,
        input: input::<reference::Input>,
        output: output::<reference::Output>,
        read_only: false,
        env: &gh::ENV,
        timeout: None,
    },
    Capability {
        name: "push",
        description: "Push the branch checked out to origin, once the boundary passes its name and \
            each commit it would send.",
        run: send::push,
        input: input::<send::PushInput>,
        output: output::<send::Pushed>,
        read_only: false,
        env: &SEND_ENV,
        timeout: Some(SEND_TIMEOUT),
    },
    Capability {
        name: "pr-create",
        description: "Push the branch checked out and open a pull request from it, once the \
            boundary passes the commits, title and body in one check, and write the pull \
            request's record.",
        run: send::pr_create,
        input: input::<send::PrCreateInput>,
        output: output::<send::Opened>,
        read_only: false,
        env: &SEND_ENV,
        timeout: Some(SEND_TIMEOUT),
    },
    Capability {
        name: "pr-edit",
        description: "Change a pull request's title or body, once the boundary passes the new text.",
        run: send::pr_edit,
        input: input::<send::PrEditInput>,
        output: output::<send::Delivered>,
        read_only: false,
        env: &SEND_ENV,
        timeout: Some(SEND_TIMEOUT),
    },
    Capability {
        name: "comment",
        description: "Comment on a pull request or issue, once the boundary passes the comment.",
        run: send::comment,
        input: input::<send::CommentInput>,
        output: output::<send::Delivered>,
        read_only: false,
        env: &SEND_ENV,
        timeout: Some(SEND_TIMEOUT),
    },
    Capability {
        name: "issue-create",
        description: "Open an issue, once the boundary passes its title and body, and write the \
            issue's record.",
        run: send::issue_create,
        input: input::<send::IssueCreateInput>,
        output: output::<send::Filed>,
        read_only: false,
        env: &SEND_ENV,
        timeout: Some(SEND_TIMEOUT),
    },
    Capability {
        name: "install-hook",
        description: "Print the command that installs a git pre-push hook in the checkout, which \
            has the boundary check what each push sends; refuse when a different pre-push hook is \
            there. It writes nothing.",
        run: hook::install_hook,
        input: input::<hook::InstallHookInput>,
        output: output::<hook::InstallHookOutput>,
        read_only: true,
        env: &[],
        timeout: None,
    },
    Capability {
        name: "pre-push",
        description: "Run by the pre-push hook: check the refs git pushes as github.push checks a \
            push, and refuse when the boundary refuses or the check cannot run.",
        run: hook::pre_push,
        input: input::<hook::PrePushInput>,
        output: output::<hook::PrePushOutput>,
        read_only: false,
        env: &PRE_PUSH_ENV,
        timeout: Some(SEND_TIMEOUT),
    },
    Capability {
        name: "covers",
        description: "Name the boundaries that cover a git push or gh write from the checkout, and \
            where it goes, checking nothing and writing nothing.",
        run: hook::covers,
        input: input::<hook::CoversInput>,
        output: output::<hook::CoversOutput>,
        read_only: true,
        env: &remote::SSH_VARIABLES,
        timeout: None,
    },
];

fn manifest() -> Value {
    let capabilities: Vec<Value> = (CAPABILITIES.iter())
        .map(|offered| {
            let mut tool = json!({
                "name": offered.name,
                "description": offered.description,
                "inputSchema": (offered.input)(),
                "outputSchema": (offered.output)(),
                "annotations": {"readOnlyHint": offered.read_only},
                "_meta": {},
            });
            if !offered.env.is_empty() {
                tool["env"] = json!(offered.env);
            }
            if let Some(timeout) = offered.timeout {
                tool["timeout"] = json!(timeout);
            }
            tool
        })
        .collect();
    json!({
        "$type": "board/plugin-manifest/v1",
        "name": "github",
        "version": "0.1.0",
        // SKILL.md's description, the callout an agent sees first.
        "description": "Record a GitHub pull request or issue as a typed record, and push, open or \
            edit pull requests, comment and open issues only after the boundary passes what they \
            send. Use it to name a pull request or issue in a record, and in place of git push and \
            gh for sending. Run `board call github.ref --ref <owner>/<repo>#<number>`, or `board \
            call github.push`, `github.pr-create`, `github.pr-edit`, `github.comment` or \
            `github.issue-create` from the checkout. SKILL.md and README.md in the plugin's \
            directory say more.",
        "metadata": {"area": "delivery"},
        "types": [PullRecord::TYPE, IssueRecord::TYPE],
        "capabilities": capabilities,
    })
}

#[cfg(test)]
mod tests;
