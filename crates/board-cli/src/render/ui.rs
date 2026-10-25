//! `board serve`, `board ui kinds`, `board ui template` and `board ui open`.

use board_model::{ComponentKind, canonical_json};
use board_serve::{Opened, Served};
use crumbkit::help::Leaf;
use crumbkit::{Document, Listing, Prose, Row};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use super::kit::{Printer, Refused, Said};
use crate::report::Output;
use crate::run::Page;

type Answer = Prose<Said, Refused>;

const SERVE: Document<Answer> = Document::new(
    "board",
    "serve-report",
    2,
    "Where the server listened and what it served before it went idle.",
);
const TEMPLATE: Document<Answer> = Document::new(
    "board",
    "ui-template-report",
    2,
    "A slot's fill for a component kind.",
);
const OPEN: Document<Answer> = Document::new(
    "board",
    "ui-open-report",
    2,
    "The page opened, and whether its server was started.",
);
const KINDS: Document<Listing<Kind>> = Document::new(
    "board",
    "ui-kinds-report",
    2,
    "The component kinds a view's slot can name.",
);

pub(super) const LEAVES: [Leaf; 4] = [
    Leaf::new(&["serve"]).document(SERVE),
    Leaf::new(&["ui", "kinds"]).document(KINDS),
    Leaf::new(&["ui", "template"]).document(TEMPLATE),
    Leaf::new(&["ui", "open"]).document(OPEN),
];

/// `serve`: the port, the requests answered and the idle time that ended it.
pub(crate) fn serve(printer: Printer, served: &Served) -> Output {
    let idle = served.idle.as_millis();
    let value = json!({"port": served.port, "requests": served.requests, "idle_ms": idle});
    let text = format!(
        "served {} requests on 127.0.0.1:{}, then stopped after {idle}ms with none\n",
        served.requests, served.port
    );
    printer.print(SERVE, Ok(Prose::new(Said::new(value, text))))
}

/// `ui template`: the fill as JSON text, ready to paste into a binding.
pub(crate) fn ui_template(printer: Printer, kind: ComponentKind) -> Output {
    let fill: Value = serde_json::from_str(kind.template()).unwrap_or(Value::Null);
    let text = format!("{}\n", canonical_json(&fill).as_str());
    let value = json!({"kind": kind.name(), "fill": fill});
    printer.print(TEMPLATE, Ok(Prose::new(Said::new(value, text))))
}

/// `ui open`: the binding opened, the port, and whether the server started.
pub(crate) fn ui_open(printer: Printer, opened: &Opened) -> Output {
    let started = if opened.started {
        ", which it started"
    } else {
        ""
    };
    let text = format!(
        "opened {} from the server on 127.0.0.1:{}{started}\n",
        opened.binding, opened.port
    );
    let value = json!({
        "binding": opened.binding.to_string(),
        "port": opened.port,
        "started": opened.started,
    });
    printer.print(OPEN, Ok(Prose::new(Said::new(value, text))))
}

/// `ui kinds`: a row per kind.
pub(crate) fn ui_kinds(printer: Printer, page: &Page) -> Output {
    let rows = ComponentKind::ALL
        .into_iter()
        .map(|kind| Kind {
            description: kind.description(),
            draws: kind.draws().join(", "),
            name: kind.name(),
            schema: kind.schema(),
        })
        .collect();
    printer.print(KINDS, Ok(page.first_of(rows).described("kinds")))
}

/// A component kind as `ui kinds` prints it.
#[derive(Serialize, JsonSchema)]
struct Kind {
    /// What the kind draws.
    description: &'static str,
    /// The record types a slot of this kind draws by `ref`; `*` is any.
    draws: String,
    /// The name a view's slot gives as its `kind`.
    name: &'static str,
    /// The JSON Schema a slot's data or referenced record must satisfy.
    schema: Value,
}

impl Row for Kind {
    const COLUMNS: &'static [&'static str] = &["name", "draws", "description"];
    const LONG_COLUMNS: &'static [&'static str] = &["schema"];
}
