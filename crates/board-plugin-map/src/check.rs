//! `check`: what a reader would see as broken in a map, from its stored
//! positions and label sides. It ports the prototype checker's tests and
//! thresholds unchanged (docs/plugins/built-in/map.md).

use std::collections::{BTreeMap, BTreeSet};

use board_model::{Code, Fields, LogicalPath, PathError, Refusal};
use board_ops::{Host, PluginError};
use board_plugin::shape::{Prose, Ref};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::geom::{
    Align, Box2, Mark, NotePoint, OFFSET, RADIUS, Segment, label_box, line_ends, line_note,
    note_box, title_box,
};
use crate::records::{Anchor, LayoutRecord, Region};

/// The map to check.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CheckInput")]
pub(crate) struct Input {
    /// The map's path, such as /maps/services.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
    /// The most findings to list; 100 when left out.
    #[schemars(range(min = 1, max = 10000))]
    limit: Option<usize>,
}

/// The findings in a map, in a stable order, and how many the limit left
/// out.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CheckOutput")]
pub(crate) struct Output {
    /// The map that was checked.
    #[schemars(with = "Ref<LayoutRecord>")]
    path: String,
    /// How many findings the map has, listed or not.
    problems: usize,
    /// The findings, by kind and then message.
    findings: Vec<Finding>,
    /// How many findings the limit left out.
    left_out: usize,
}

/// One thing a reader would see as broken.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub kind: Kind,
    /// What is wrong, naming the items, lines, notes and regions involved.
    #[schemars(with = "Prose")]
    pub message: String,
}

// What kind of problem a finding is. No doc comment, which would describe
// the inlined enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(inline)]
pub enum Kind {
    ItemUnplaced,
    ItemsOverlap,
    LabelNearerOther,
    LabelOffSheet,
    LabelOutsideRegion,
    LabelOverlap,
    LineThroughItem,
    LineThroughLabel,
    LinesShareAngle,
    NoteAdrift,
    NoteNearerOther,
    NoteOverlap,
}

/// How near in angle, in radians, two lines leaving one item may be.
const SHARED_ANGLE: f64 = 0.14;
/// How far a note may be from what it describes.
const REACH: f64 = 70.0;

pub(crate) fn call(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { path, limit } = PluginError::read_input("map", "check", input)?;
    let map = read(host, &path)?;
    let mut findings = check(&map)?;
    findings.sort();
    let problems = findings.len();
    let limit = limit.unwrap_or(100);
    let left_out = problems.saturating_sub(limit);
    findings.truncate(limit);
    let output = Output {
        path,
        problems,
        findings,
        left_out,
    };
    Ok(serde_json::to_value(output).expect("the output is strings and counts"))
}

/// The map at `path`.
fn read(host: &dyn Host, path: &str) -> Result<LayoutRecord, PluginError> {
    let logical: LogicalPath = path.parse().map_err(|error: PathError| {
        let refusal = Refusal::new(error.code(), &error, &Fields::default());
        PluginError::board_refused("map", "get", &refusal)
    })?;
    let refused = |refusal: &Refusal| PluginError::board_refused("map", "get", refusal);
    let answers = (host.get_each(&[logical])).map_err(|error| refused(&error.refusal("get")))?;
    let record = match answers.into_iter().next() {
        Some(Ok(report)) => report.record.map_err(|refusal| refused(&refusal))?,
        Some(Err(error)) if error.code() == Code::PathNotFound => {
            return Err(refusal(
                "map::layout::not_found",
                format!("no map is at {path}"),
                "name a map/layout/v1 record; `board ls /maps` lists the maps",
                [("path", path)],
            ));
        }
        Some(Err(error)) => return Err(refused(&error.refusal("get"))),
        None => {
            return Err(refusal(
                "map::board::refused",
                format!("board get gave no answer for {path}"),
                "run `board get` on the path to see why",
                [("path", path)],
            ));
        }
    };
    serde_json::from_value(record).map_err(|error| {
        refusal(
            "map::layout::unreadable",
            format!("{path} is not a map/layout/v1 record: {error}"),
            "name a map/layout/v1 record, or run `board check` on the path and fix what it names",
            [("path", path)],
        )
    })
}

fn refusal<const N: usize>(
    code: &str,
    message: String,
    hint: &str,
    fields: [(&str, &str); N],
) -> PluginError {
    PluginError {
        code: code.to_owned(),
        message,
        fields: Map::from_iter(fields.map(|(name, value)| (name.to_owned(), json!(value)))),
        hint: Some(hint.to_owned()),
    }
}

/// A placed item as the checker sees it.
struct Placed<'a> {
    id: &'a str,
    mark: Mark,
    label: Box2,
    region: Option<&'a Region>,
}

/// A drawn line.
struct Drawn<'a> {
    from: &'a str,
    to: &'a str,
    segment: Segment,
}

impl Drawn<'_> {
    fn name(&self) -> String {
        format!("the line {}→{}", self.from, self.to)
    }
}

/// A drawn note.
struct Written<'a> {
    text: &'a str,
    area: Box2,
    on: On<'a>,
}

/// What a drawn note belongs to.
enum On<'a> {
    Item(usize),
    Line(&'a str, &'a str),
}

/// Every finding in `map`, in no particular order. A map that names an
/// item, region or line it does not hold, or holds two of one name, is
/// refused.
pub fn check(map: &LayoutRecord) -> Result<Vec<Finding>, PluginError> {
    known(map)?;
    let mut found = Vec::new();
    let mut add = |kind, message: String| found.push(Finding { kind, message });
    let regions: BTreeMap<&str, &Region> =
        (map.regions.iter()).map(|region| (region.id.as_str(), region)).collect();
    let mut placed = Vec::new();
    for item in &map.items {
        let Some(at) = item.at else {
            add(
                Kind::ItemUnplaced,
                format!("{} has no stored position, so the map does not draw it", item.id),
            );
            continue;
        };
        let mark = Mark {
            x: at.x,
            y: at.y,
            r: item.radius.unwrap_or(RADIUS),
        };
        placed.push(Placed {
            id: &item.id,
            mark,
            label: label_box(&mark, item.side.unwrap_or_default(), &item.label),
            region: item.region.as_deref().and_then(|id| regions.get(id).copied()),
        });
    }
    let index: BTreeMap<&str, usize> =
        (placed.iter().enumerate()).map(|(at, item)| (item.id, at)).collect();
    let lines: Vec<Drawn> = (map.lines.iter())
        .filter_map(|line| {
            let (a, b) = (index.get(line.from.as_str())?, index.get(line.to.as_str())?);
            Some(Drawn {
                from: &line.from,
                to: &line.to,
                segment: line_ends(&placed[*a].mark, &placed[*b].mark),
            })
        })
        .collect();
    let notes = written(map, &placed, &index);
    labels(&placed, &mut add);
    crossings(&placed, &lines, &index, &mut add);
    nearness(&placed, &lines, &notes, &mut add);
    sheet(map, &placed, &lines, &mut add);
    note_clashes(&placed, &lines, &notes, &mut add);
    Ok(found)
}

/// Refuses a map whose items or regions repeat a name, or whose items,
/// lines or notes name what it does not hold.
fn known(map: &LayoutRecord) -> Result<(), PluginError> {
    let mut items = BTreeSet::new();
    let mut regions = BTreeSet::new();
    let repeated = |what: &str, id: &str| {
        refusal(
            "map::layout::duplicate_id",
            format!("the map holds two {what}s named {id}"),
            "give each item and each region a name of its own",
            [("id", id)],
        )
    };
    for region in &map.regions {
        if !regions.insert(region.id.as_str()) {
            return Err(repeated("region", &region.id));
        }
    }
    for item in &map.items {
        if !items.insert(item.id.as_str()) {
            return Err(repeated("item", &item.id));
        }
    }
    let unknown = |what: &str, id: &str, by: String| {
        refusal(
            "map::layout::unknown_id",
            format!("{by} names the {what} {id}, which the map does not hold"),
            "add it to the map, or name one the map holds",
            [("id", id)],
        )
    };
    for item in &map.items {
        if let Some(region) = item.region.as_deref().filter(|id| !regions.contains(id)) {
            return Err(unknown("region", region, format!("the item {}", item.id)));
        }
    }
    let mut lines = BTreeSet::new();
    for line in &map.lines {
        for end in [&line.from, &line.to] {
            if !items.contains(end.as_str()) {
                return Err(unknown("item", end, format!("the line {}→{}", line.from, line.to)));
            }
        }
        lines.insert((line.from.as_str(), line.to.as_str()));
    }
    for note in &map.notes {
        let by = || format!("the note \"{}\"", note.text);
        match &note.anchor {
            Anchor::Item(on) if !items.contains(on.id.as_str()) => {
                return Err(unknown("item", &on.id, by()));
            }
            Anchor::Line(on) if !lines.contains(&(on.from.as_str(), on.to.as_str())) => {
                let line = format!("{}→{}", on.from, on.to);
                return Err(unknown("line", &line, by()));
            }
            Anchor::Item(_) | Anchor::Line(_) => {}
        }
    }
    Ok(())
}

/// Each note whose anchor is drawn, with its box.
fn written<'a>(
    map: &'a LayoutRecord,
    placed: &[Placed<'a>],
    index: &BTreeMap<&str, usize>,
) -> Vec<Written<'a>> {
    (map.notes.iter())
        .filter_map(|note| {
            let (point, on) = match &note.anchor {
                Anchor::Item(on) => {
                    let at = *index.get(on.id.as_str())?;
                    let mark = placed[at].mark;
                    let point = NotePoint {
                        x: mark.x + on.dx,
                        y: mark.y + on.dy,
                        align: Align::Start,
                    };
                    (point, On::Item(at))
                }
                Anchor::Line(on) => {
                    let a = placed[*index.get(on.from.as_str())?].mark;
                    let b = placed[*index.get(on.to.as_str())?].mark;
                    let offset = on.offset.unwrap_or(OFFSET);
                    let point = line_note(&a, &b, on.along, offset);
                    (point, On::Line(&on.from, &on.to))
                }
            };
            Some(Written {
                text: &note.text,
                area: note_box(&point, &note.text),
                on,
            })
        })
        .collect()
}

/// Labels over labels or marks, and marks over marks.
fn labels(placed: &[Placed], add: &mut impl FnMut(Kind, String)) {
    for (at, one) in placed.iter().enumerate() {
        for other in &placed[at + 1..] {
            if one.label.hits(&other.label) {
                let message = format!(
                    "the label of {} overlaps the label of {}",
                    one.id, other.id
                );
                add(Kind::LabelOverlap, message);
            }
        }
    }
    for one in placed {
        for other in placed.iter().filter(|other| other.id != one.id) {
            if one.label.hits(&other.mark.square()) {
                let message = format!("the label of {} overlaps the mark of {}", one.id, other.id);
                add(Kind::LabelOverlap, message);
            }
        }
    }
    for (at, a) in placed.iter().enumerate() {
        for b in &placed[at + 1..] {
            let apart = (a.mark.x - b.mark.x).hypot(a.mark.y - b.mark.y);
            if apart < a.mark.r + b.mark.r + 8.0 {
                add(
                    Kind::ItemsOverlap,
                    format!("the marks of {} and {} overlap", a.id, b.id),
                );
            }
        }
    }
}

/// Lines through labels or marks, and lines leaving one item at nearly one
/// angle.
fn crossings<'a>(
    placed: &[Placed],
    lines: &[Drawn<'a>],
    index: &BTreeMap<&str, usize>,
    add: &mut impl FnMut(Kind, String),
) {
    for line in lines {
        for item in placed {
            if line.segment.crosses(&item.label) {
                let message = format!("{} crosses the label of {}", line.name(), item.id);
                add(Kind::LineThroughLabel, message);
            }
        }
        for item in placed.iter().filter(|item| item.id != line.from && item.id != line.to) {
            if line.segment.distance(item.mark.x, item.mark.y) < item.mark.r + 4.0 {
                let message = format!("{} passes over the mark of {}", line.name(), item.id);
                add(Kind::LineThroughItem, message);
            }
        }
    }
    let mark = |id: &str| index.get(id).map(|at| placed[*at].mark);
    for (at, a) in lines.iter().enumerate() {
        for b in &lines[at + 1..] {
            let Some(shared) = [a.from, a.to].into_iter().find(|id| *id == b.from || *id == b.to)
            else {
                continue;
            };
            let far = |line: &Drawn<'a>| if line.from == shared { line.to } else { line.from };
            let (Some(s), Some(p), Some(q)) = (mark(shared), mark(far(a)), mark(far(b))) else {
                continue;
            };
            let one = (p.y - s.y).atan2(p.x - s.x);
            let two = (q.y - s.y).atan2(q.x - s.x);
            let mut apart = (one - two).abs();
            if apart > std::f64::consts::PI {
                apart = 2.0 * std::f64::consts::PI - apart;
            }
            if apart < SHARED_ANGLE {
                let message = format!(
                    "the lines {}→{} and {}→{} leave one item at nearly one angle",
                    a.from, a.to, b.from, b.to
                );
                add(Kind::LinesShareAngle, message);
            }
        }
    }
}

/// The distance from `area` to the edge of `mark`.
fn reach(area: &Box2, mark: &Mark) -> f64 {
    area.distance(mark.x, mark.y) - mark.r
}

/// Of `candidates`, each a distance, a name and a part, the nearest part
/// more than 2 units nearer than `own`, ties broken by name, so the answer
/// does not depend on the order the map lists its parts in. The prototype
/// named the first such part instead.
fn nearest<T>(candidates: impl Iterator<Item = (f64, String, T)>, own: f64) -> Option<T> {
    (candidates.filter(|(distance, _, _)| *distance < own - 2.0))
        .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, _, part)| part)
}

/// Labels outside their region, and labels and notes nearer another mark
/// or line than their own.
fn nearness(
    placed: &[Placed],
    lines: &[Drawn],
    notes: &[Written],
    add: &mut impl FnMut(Kind, String),
) {
    for item in placed {
        if let Some(region) = item.region {
            let area = Box2 {
                x0: region.x,
                y0: region.y,
                x1: region.x + region.width,
                y1: region.y + region.height,
            };
            if !item.label.within(&area) {
                let message = format!("the label of {} leaves its region", item.id);
                add(Kind::LabelOutsideRegion, message);
            }
        }
        let own = reach(&item.label, &item.mark);
        let others = (placed.iter())
            .filter(|other| other.id != item.id)
            .map(|other| (reach(&item.label, &other.mark), other.id.to_owned(), other));
        if let Some(other) = nearest(others, own) {
            let message = format!(
                "the label of {} is nearer the mark of {} than its own",
                item.id, other.id
            );
            add(Kind::LabelNearerOther, message);
        }
    }
    for note in notes {
        let On::Line(from, to) = note.on else {
            continue;
        };
        let (cx, cy) = note.area.centre();
        let own = (lines.iter())
            .find(|line| line.from == from && line.to == to)
            .map_or(f64::INFINITY, |line| line.segment.distance(cx, cy));
        let others = (lines.iter())
            .filter(|line| !(line.from == from && line.to == to))
            .map(|line| (line.segment.distance(cx, cy), line.name(), line));
        if let Some(line) = nearest(others, own) {
            let message = format!(
                "the note \"{}\" on the line {from}→{to} is nearer {}",
                note.text,
                line.name()
            );
            add(Kind::NoteNearerOther, message);
        }
    }
    for note in notes {
        let On::Item(at) = note.on else {
            continue;
        };
        let own = reach(&note.area, &placed[at].mark);
        let others = (placed.iter().enumerate())
            .filter(|(other, _)| *other != at)
            .map(|(_, other)| (reach(&note.area, &other.mark), other.id.to_owned(), other));
        if let Some(other) = nearest(others, own) {
            let message = format!(
                "the note \"{}\" is nearer the mark of {} than its own item",
                note.text, other.id
            );
            add(Kind::NoteNearerOther, message);
        }
    }
}

/// Labels off the sheet, and labels and lines over region titles.
fn sheet(
    map: &LayoutRecord,
    placed: &[Placed],
    lines: &[Drawn],
    add: &mut impl FnMut(Kind, String),
) {
    let page = Box2 {
        x0: 0.0,
        y0: 0.0,
        x1: map.sheet.width,
        y1: map.sheet.height,
    };
    for item in placed.iter().filter(|item| !item.label.within(&page)) {
        add(
            Kind::LabelOffSheet,
            format!("the label of {} leaves the sheet", item.id),
        );
    }
    for region in &map.regions {
        let title = title_box(region.x, region.y, &region.title);
        for item in placed.iter().filter(|item| item.label.hits(&title)) {
            let message = format!(
                "the label of {} overlaps the title of region {}",
                item.id, region.id
            );
            add(Kind::LabelOverlap, message);
        }
        for line in lines.iter().filter(|line| line.segment.crosses(&title)) {
            let message = format!("{} crosses the title of region {}", line.name(), region.id);
            add(Kind::LineThroughLabel, message);
        }
    }
}

/// Notes far from what they describe, and notes over labels, marks or
/// other lines.
fn note_clashes(
    placed: &[Placed],
    lines: &[Drawn],
    notes: &[Written],
    add: &mut impl FnMut(Kind, String),
) {
    let mark = |id: &str| placed.iter().find(|item| item.id == id).map(|item| item.mark);
    for note in notes {
        let target = match note.on {
            On::Item(at) => Some(((placed[at].mark.x, placed[at].mark.y), placed[at].id.to_owned())),
            On::Line(from, to) => mark(from).zip(mark(to)).map(|(a, b)| {
                let middle = (f64::midpoint(a.x, b.x), f64::midpoint(a.y, b.y));
                (middle, format!("the line {from}→{to}"))
            }),
        };
        if let Some(((tx, ty), name)) = target {
            let distance = note.area.distance(tx, ty);
            if distance > REACH {
                let units = (distance + 0.5).floor();
                let message = format!("the note \"{}\" is {units} units from {name}", note.text);
                add(Kind::NoteAdrift, message);
            }
        }
        for item in placed.iter().filter(|item| note.area.hits(&item.label)) {
            let message = format!("the note \"{}\" overlaps the label of {}", note.text, item.id);
            add(Kind::NoteOverlap, message);
        }
        for item in placed.iter().filter(|item| note.area.hits(&item.mark.square())) {
            let message = format!("the note \"{}\" overlaps the mark of {}", note.text, item.id);
            add(Kind::NoteOverlap, message);
        }
        for line in lines {
            if let On::Line(from, to) = note.on
                && line.from == from
                && line.to == to
            {
                continue;
            }
            if line.segment.crosses(&note.area) {
                let message = format!("the note \"{}\" overlaps {}", note.text, line.name());
                add(Kind::NoteOverlap, message);
            }
        }
    }
}

#[cfg(test)]
mod tests;
