//! The page: one HTML file that holds its style, its three data blocks and
//! its code, and fetches nothing when it opens. The assets are in `page/`.

use serde_json::Value;

use crate::model::Page;

const STYLE: &str = include_str!("../page/style.css");
const SHELL: &str = include_str!("../page/page.html");
/// The code, in the order it runs.
const SCRIPTS: [&str; 3] = [
    include_str!("../page/01-model.js"),
    include_str!("../page/02-view.js"),
    include_str!("../page/03-ui.js"),
];

/// A block as a script element of JSON, which the page reads by its id. The
/// text has `</` escaped so that it cannot end the element.
fn block(id: &str, value: &Value) -> String {
    let json = serde_json::to_string(value).expect("a value serialises");
    format!(
        "<script type=\"application/json\" id=\"{id}\">\n{}\n</script>\n",
        json.replace("</", "<\\/")
    )
}

/// The page's HTML. It is a function of the blocks alone.
pub(crate) fn html(page: &Page) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<meta charset=\"utf-8\">\n<title>Work</title>\n\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
         <style>\n{STYLE}</style>\n{SHELL}{}{}{}<script>\n'use strict';\n{}\n</script>\n</html>\n",
        block("data", &page.data),
        block("systems", &page.systems),
        block("questions", &page.questions),
        SCRIPTS.join("\n"),
    )
}

#[cfg(test)]
mod tests;
