//! `page`: one self-contained HTML page for a map, drawn from its stored
//! positions with the geometry the checker uses, so a finding-free map
//! reads as the checker said it would. The page holds no script source, no
//! link and no font file besides a few lines that light an item and its
//! lines when the reader points at it.

use std::fmt::Write as _;

use board_ops::{Host, PluginError};
use board_plugin::shape::Ref;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::check::{check, read};
use crate::geom::{Align, Mark, NotePoint, OFFSET, RADIUS, label_box, line_ends, line_note};
use crate::records::{Anchor, LayoutRecord, LineKind, Side, Stroke};

/// The map to draw.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PageInput")]
pub(crate) struct Input {
    /// The map's path, such as /maps/services.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
}

/// The page, and the items it left out.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PageOutput")]
pub(crate) struct Output {
    /// The map that was drawn.
    #[schemars(with = "Ref<LayoutRecord>")]
    path: String,
    /// The items with no stored position, which the page does not draw.
    unplaced: Vec<String>,
    /// The page: one HTML document that makes no network request. Save it
    /// as a file to publish it.
    html: String,
}

pub(crate) fn call(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path } = PluginError::read_input("map", "page", input)?;
    let map = read(host, &path)?;
    check(&map)?;
    let unplaced = (map.items.iter())
        .filter(|item| item.at.is_none())
        .map(|item| item.id.clone())
        .collect();
    let output = Output {
        path,
        unplaced,
        html: render(&map),
    };
    Ok(serde_json::to_value(output).expect("the output is strings"))
}

/// `text` as HTML text or an attribute value.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The number as the shortest text that keeps a tenth.
fn num(value: f64) -> String {
    let text = format!("{:.1}", (value * 10.0).round() / 10.0);
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

/// The shade of bin `bin` of `bins`, spread over the five the page holds.
fn shade(bin: u8, bins: usize) -> usize {
    if bins < 2 {
        return 1;
    }
    1 + (usize::from(bin) * 8 / (bins - 1)).div_ceil(2)
}

/// The kind's entry, or the one a kind with none gets.
fn kind_of<'a>(map: &'a LayoutRecord, kind: &str) -> (Option<&'a LineKind>, Stroke, bool) {
    let entry = map.line_kinds.iter().find(|entry| entry.kind == kind);
    (
        entry,
        entry.and_then(|entry| entry.style).unwrap_or(Stroke::Solid),
        entry.and_then(|entry| entry.arrow).unwrap_or(true),
    )
}

fn dash(stroke: Stroke) -> &'static str {
    match stroke {
        Stroke::Solid => "none",
        Stroke::Dashed => "6 4",
        Stroke::Dotted => "2 4",
    }
}

/// The page for `map`.
pub(crate) fn render(map: &LayoutRecord) -> String {
    let title = esc(&map.title);
    let mut out = String::new();
    let _ = write!(
        out,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<style>{STYLE}</style>\n</head>\n\
         <body>\n<div class=\"page\">\n<header><h1>{title}</h1></header>\n\
         <section class=\"block\" aria-labelledby=\"h-map\">\n<h2 id=\"h-map\">Map</h2>\n"
    );
    draw(map, &mut out);
    key(map, &mut out);
    out.push_str("</section>\n");
    for table in &map.tables {
        tabulate(table, &mut out);
    }
    out.push_str("</div>\n<script>");
    out.push_str(SCRIPT);
    out.push_str("</script>\n</body>\n</html>\n");
    out
}

/// A placed item for drawing.
struct Spot<'a> {
    item: &'a crate::records::Item,
    mark: Mark,
}

fn draw(map: &LayoutRecord, out: &mut String) {
    let spots: Vec<Spot> = (map.items.iter())
        .filter_map(|item| {
            let at = item.at?;
            let mark = Mark {
                x: at.x,
                y: at.y,
                r: item.radius.unwrap_or(RADIUS),
            };
            Some(Spot { item, mark })
        })
        .collect();
    let _ = write!(
        out,
        "<div class=\"mapwrap\"><svg class=\"map\" viewBox=\"0 0 {} {}\" role=\"img\" aria-label=\"{}\">\n\
         <defs><marker id=\"arr\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"8\" markerHeight=\"8\" \
         orient=\"auto-start-reverse\" markerUnits=\"userSpaceOnUse\"><path d=\"M0,1 L9,5 L0,9 z\" fill=\"var(--ink-2)\"/></marker></defs>\n",
        num(map.sheet.width),
        num(map.sheet.height),
        esc(&map.title)
    );
    regions(map, out);
    lines(map, &spots, out);
    items(map, &spots, out);
    notes(map, &spots, out);
    out.push_str("</svg></div>\n");
}

fn regions(map: &LayoutRecord, out: &mut String) {
    for (at, region) in map.regions.iter().enumerate() {
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"14\" fill=\"var(--region-{})\"/>\
             <text x=\"{}\" y=\"{}\" class=\"sys\">{}</text>",
            num(region.x),
            num(region.y),
            num(region.width),
            num(region.height),
            ["a", "b", "c", "d", "e"][at % 5],
            num(region.x + 12.0),
            num(region.y + 18.0),
            esc(&region.title)
        );
    }
}

fn lines(map: &LayoutRecord, spots: &[Spot], out: &mut String) {
    let find = |id: &str| spots.iter().find(|spot| spot.item.id == id);
    for line in &map.lines {
        let (Some(a), Some(b)) = (find(&line.from), find(&line.to)) else {
            continue;
        };
        let (entry, stroke, arrow) = kind_of(map, &line.kind);
        let seg = line_ends(&a.mark, &b.mark);
        let _ = writeln!(
            out,
            "<path class=\"edge\" d=\"M{},{} L{},{}\" stroke-dasharray=\"{}\"{} data-a=\"{}\" data-b=\"{}\"><title>{}</title></path>",
            num(seg.x1),
            num(seg.y1),
            num(seg.x2),
            num(seg.y2),
            dash(stroke),
            if arrow {
                " marker-end=\"url(#arr)\""
            } else {
                ""
            },
            esc(&line.from),
            esc(&line.to),
            esc(&format!(
                "{} {} {}",
                line.from,
                entry.map_or(line.kind.as_str(), |entry| entry.label.as_str()),
                line.to
            ))
        );
    }
}

fn items(map: &LayoutRecord, spots: &[Spot], out: &mut String) {
    let bins = map.ramp.as_ref().map_or(0, |ramp| ramp.bins.len());
    for spot in spots {
        let fill = spot.item.bin.map_or_else(
            || "var(--line)".to_owned(),
            |bin| format!("var(--cx-{})", shade(bin, bins)),
        );
        let side = spot.item.side.unwrap_or_default();
        let area = label_box(&spot.mark, side, &spot.item.label);
        let (x, anchor) = match side {
            Side::Right => (area.x0, "start"),
            Side::Left => (area.x1, "end"),
            Side::Below | Side::Above => (f64::midpoint(area.x0, area.x1), "middle"),
        };
        let _ = write!(
            out,
            "<g class=\"node\" data-id=\"{}\" tabindex=\"0\"><circle class=\"mark\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\"/>",
            esc(&spot.item.id),
            num(spot.mark.x),
            num(spot.mark.y),
            num(spot.mark.r)
        );
        for (index, text) in spot.item.label.iter().enumerate() {
            let _ = write!(
                out,
                "<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" class=\"{}\">{}</text>",
                num(x),
                num(area.y0 + 11.0 + 15.0 * f64::from(u32::try_from(index).unwrap_or(0))),
                if index == 0 { "name" } else { "dim" },
                esc(text)
            );
        }
        let _ = writeln!(
            out,
            "<title>{}</title></g>",
            esc(&spot.item.label.join(", "))
        );
    }
}

fn notes(map: &LayoutRecord, spots: &[Spot], out: &mut String) {
    let find = |id: &str| spots.iter().find(|spot| spot.item.id == id);
    for note in &map.notes {
        let point = match &note.anchor {
            Anchor::Item(on) => find(&on.id).map(|spot| NotePoint {
                x: spot.mark.x + on.dx,
                y: spot.mark.y + on.dy,
                align: Align::Start,
            }),
            Anchor::Line(on) => find(&on.from)
                .zip(find(&on.to))
                .map(|(a, b)| line_note(&a.mark, &b.mark, on.along, on.offset.unwrap_or(OFFSET))),
        };
        let Some(point) = point else { continue };
        let anchor = match point.align {
            Align::Start => "start",
            Align::Middle => "middle",
            Align::End => "end",
        };
        let _ = writeln!(
            out,
            "<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" class=\"note\">{}</text>",
            num(point.x),
            num(point.y),
            esc(&note.text)
        );
    }
}

/// The key: the ramp with its bins, then each line kind the map draws.
fn key(map: &LayoutRecord, out: &mut String) {
    out.push_str("<div class=\"key\">");
    if let Some(ramp) = &map.ramp {
        let _ = write!(
            out,
            "<span class=\"item\"><span>{}</span><span class=\"ramp\" style=\"grid-template-columns:repeat({}, auto)\">",
            esc(&ramp.label),
            ramp.bins.len()
        );
        for bin in 0..ramp.bins.len() {
            let _ = write!(
                out,
                "<span class=\"sw\" style=\"background:var(--cx-{})\"></span>",
                shade(u8::try_from(bin).unwrap_or(0), ramp.bins.len())
            );
        }
        for bin in &ramp.bins {
            let _ = write!(out, "<span>{}</span>", esc(bin));
        }
        out.push_str("</span></span>");
    }
    let mut kinds: Vec<&str> = map.lines.iter().map(|line| line.kind.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    for kind in kinds {
        let (entry, stroke, arrow) = kind_of(map, kind);
        let _ = write!(
            out,
            "<span class=\"item\"><svg width=\"34\" height=\"10\" aria-hidden=\"true\"><path d=\"M2,5 L32,5\" stroke=\"var(--ink-2)\" \
             stroke-width=\"1.6\" stroke-dasharray=\"{}\" stroke-linecap=\"round\" fill=\"none\"/>{}</svg>{}</span>",
            dash(stroke),
            if arrow {
                "<path d=\"M26,2 L32,5 L26,8\" stroke=\"var(--ink-2)\" stroke-width=\"1.6\" fill=\"none\"/>"
            } else {
                ""
            },
            esc(entry.map_or(kind, |entry| entry.label.as_str()))
        );
    }
    out.push_str("</div>\n");
}

fn tabulate(table: &crate::records::Table, out: &mut String) {
    let _ = write!(
        out,
        "<section class=\"block\" aria-labelledby=\"t-{id}\"><h2 id=\"t-{id}\">{}</h2>\n\
         <div class=\"tablewrap\"><table><thead><tr>",
        esc(&table.title),
        id = esc(&table.id)
    );
    for column in &table.columns {
        let class = if column.numeric == Some(true) {
            " class=\"num\""
        } else {
            ""
        };
        let _ = write!(out, "<th{class}>{}</th>", esc(&column.label));
    }
    out.push_str("</tr></thead><tbody>");
    for row in &table.rows {
        match &row.item {
            Some(item) => {
                let _ = write!(out, "<tr data-id=\"{}\">", esc(item));
            }
            None => out.push_str("<tr>"),
        }
        for (cell, column) in row.cells.iter().zip(&table.columns) {
            let class = if column.numeric == Some(true) {
                " class=\"num\""
            } else {
                ""
            };
            let _ = write!(out, "<td{class}>{}</td>", esc(cell));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div>");
    if let Some(caption) = &table.caption {
        let _ = write!(out, "<p class=\"caption\">{}</p>", esc(caption));
    }
    out.push_str("</section>\n");
}

const STYLE: &str = "
:root { --page:#eef0ed; --block:#fcfcfb; --ink:#0b0b0b; --ink-2:#52514e; --ink-3:#6f6e69; --line:#c3c2b7; --hair:#e1e0d9;
 --region-a:oklch(0.955 0.03 85); --region-b:oklch(0.955 0.03 160); --region-c:oklch(0.955 0.03 320); --region-d:oklch(0.955 0.02 30); --region-e:oklch(0.955 0.025 240);
 --cx-1:#b7d9f2; --cx-2:#99abeb; --cx-3:#a46fc4; --cx-4:#ab3573; --cx-5:#8b1210; --mark-edge:rgba(11,11,11,0.35); --focus:#2a78d6;
 --font:system-ui,-apple-system,'Segoe UI',sans-serif; color-scheme:light; }
@media (prefers-color-scheme: dark) { :root { --page:#0f100f; --block:#1a1a19; --ink:#f0efec; --ink-2:#c3c2b7; --ink-3:#9a9890; --line:#4a4a46; --hair:#2c2c2a;
 --region-a:oklch(0.255 0.025 85); --region-b:oklch(0.255 0.025 160); --region-c:oklch(0.255 0.025 320); --region-d:oklch(0.255 0.02 30); --region-e:oklch(0.255 0.025 240);
 --cx-1:#204263; --cx-2:#515496; --cx-3:#9b5eae; --cx-4:#de709e; --cx-5:#fd9f81; --mark-edge:rgba(255,255,255,0.35); --focus:#6da7ec; color-scheme:dark; } }
body { margin:0; background:var(--page); color:var(--ink); font-family:var(--font); font-size:14px; line-height:1.5; }
.page { max-width:1080px; margin:0 auto; padding:24px 14px 44px; display:grid; grid-template-columns:minmax(0,1fr); gap:18px; }
h1 { font-size:24px; line-height:1.25; font-weight:600; margin:0; } h2 { font-size:16px; font-weight:600; margin:0; }
.block { background:var(--block); border-radius:18px; padding:16px; display:grid; gap:10px; min-width:0; }
.caption { color:var(--ink-3); font-size:12.5px; margin:0; } .mapwrap { overflow-x:auto; }
svg.map { width:100%; height:auto; display:block; font-family:var(--font); }
svg.map .sys { font-size:11.5px; letter-spacing:0.06em; text-transform:uppercase; fill:var(--ink-3); font-weight:500; }
svg.map text.name { font-size:14px; font-weight:500; fill:var(--ink); } svg.map text.dim { font-size:12.5px; fill:var(--ink-3); }
svg.map text.note { font-size:12.5px; fill:var(--ink-2); } svg.map .mark { stroke:var(--mark-edge); stroke-width:1; }
svg.map .edge { stroke:var(--ink-2); stroke-width:1.6; stroke-opacity:0.85; fill:none; stroke-linecap:round; }
svg.map .node:focus { outline:none; } svg.map .node.hot circle.mark { stroke:var(--focus); stroke-width:3; } svg.map .edge.cold { opacity:0.15; }
@media (max-width:640px) { .mapwrap svg.map { min-width:620px; } .tablewrap table { min-width:600px; } }
.key { display:flex; flex-wrap:wrap; align-items:center; gap:8px 18px; font-size:12.5px; color:var(--ink-2); }
.key .item { display:inline-flex; align-items:center; gap:6px; }
.ramp { display:inline-grid; gap:2px; font-size:11.5px; color:var(--ink-3); text-align:center; }
.ramp .sw { display:block; width:40px; height:12px; border-radius:3px; box-shadow:inset 0 0 0 1px var(--mark-edge); }
table { width:100%; border-collapse:collapse; font-size:13px; font-variant-numeric:tabular-nums; }
th { text-align:left; font-weight:500; color:var(--ink-3); font-size:12px; padding:4px 8px 6px; border-bottom:1px solid var(--line); }
td { padding:6px 8px; border-bottom:1px solid var(--hair); vertical-align:top; } td.num, th.num { text-align:right; }
tr.hot td { background:color-mix(in oklab, var(--focus) 12%, transparent); } .tablewrap { overflow-x:auto; }
";

const SCRIPT: &str = "
(function () {
  var hot = function (id) {
    document.querySelectorAll('.node').forEach(function (g) { g.classList.toggle('hot', g.dataset.id === id); });
    document.querySelectorAll('tr[data-id]').forEach(function (r) { r.classList.toggle('hot', r.dataset.id === id); });
    document.querySelectorAll('.edge').forEach(function (p) { p.classList.toggle('cold', !!id && p.dataset.a !== id && p.dataset.b !== id); });
  };
  document.querySelectorAll('.node, tr[data-id]').forEach(function (e) {
    e.addEventListener('pointerenter', function () { hot(e.dataset.id); });
    e.addEventListener('pointerleave', function () { hot(null); });
    e.addEventListener('focus', function () { hot(e.dataset.id); });
    e.addEventListener('blur', function () { hot(null); });
  });
})();
";

#[cfg(test)]
mod tests;
