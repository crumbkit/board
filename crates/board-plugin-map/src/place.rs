//! `place`: chooses each unplaced item's position and label side together,
//! and each line note's place along its line, by scoring every candidate
//! with the checker, and writes the whole map in one `put`. It replaces the
//! prototype's `sides.js` and `nudge.js`, which scored by the distance
//! between centres alone (docs/plugins/built-in/map.md).

use board_ops::{Host, PluginError};
use board_plugin::shape::Ref;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::check::{Finding, Kind, accept, check, read_held, refusal};
use crate::geom::{Mark, RADIUS, label_box};
use crate::records::{Anchor, At, Item, LayoutRecord, Set, Side};

/// The map to place.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlaceInput")]
pub(crate) struct Input {
    /// The map's path, such as /maps/services.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: String,
    /// Choose again the position of each item the engine placed before,
    /// and the place of each line note. Pinned items never move; false
    /// when left out.
    relayout: Option<bool>,
    /// The most findings left to list; 100 when left out.
    #[schemars(range(min = 1, max = 10000))]
    limit: Option<usize>,
}

/// What a run placed, and what it left.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PlaceOutput")]
pub(crate) struct Output {
    /// The map that was placed.
    #[schemars(with = "Ref<LayoutRecord>")]
    path: String,
    /// The items that had no stored position and were given one; an item
    /// the run found no room for is not listed.
    placed: Vec<String>,
    /// The items the engine had placed before whose position changed.
    moved: Vec<String>,
    /// The items whose label side changed.
    sides_changed: Vec<String>,
    /// How many line notes were given a new place along their line.
    notes_placed: usize,
    /// How many findings the placed map has, which is 0 when the run
    /// succeeds.
    problems: usize,
    /// The findings, by kind and then message.
    findings: Vec<Finding>,
    /// How many findings the limit left out.
    left_out: usize,
}

/// The side order candidates are tried in, and so the order ties break in.
const SIDES: [Side; 4] = [Side::Right, Side::Left, Side::Below, Side::Above];
/// The grid candidate positions lie on.
const STEP: f64 = 10.0;
/// How far a refining pass moves an item from where it is.
const REFINE: f64 = 60.0;
/// How many refining passes run.
const PASSES: usize = 3;
/// How many steps annealing takes at most.
const ANNEALING: usize = 60_000;
/// How many steps one cooling takes; annealing starts again from the best
/// map seen after each.
const COOLING: usize = 5_000;
/// The seed of annealing's random numbers.
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;
/// The gaps kept between a mark's centre and its region's edges.
const SIDE_GAP: f64 = 16.0;
const TOP_GAP: f64 = 28.0;
const BOTTOM_GAP: f64 = 12.0;
/// Where along a line a note may sit, the stored place first.
const ALONGS: [f64; 6] = [0.5, 0.35, 0.65, 0.2, 0.8, 0.45];
/// How far across its line a note may sit, in preference order.
const OFFSETS: [f64; 6] = [9.0, -10.0, 16.0, -16.0, 24.0, -24.0];

pub(crate) fn call(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        relayout,
        limit,
    } = PluginError::read_input("map", "place", input)?;
    let (mut map, blob) = read_held(host, &path)?;
    let before = serde_json::to_value(&map).expect("a map serializes");
    check(&map)?;
    let relayout = relayout.unwrap_or(false);
    let mut run = Run::new(&map, relayout);
    run.place(&mut map);
    let after = serde_json::to_value(&map).expect("a map serializes");
    if after != before {
        let line = json!({"path": path, "expect": blob, "record": after});
        (host.put(&[line]))
            .map_err(|error| PluginError::board_refused("map", "put --batch", &error.refusal()))?;
    }
    let (mut findings, _) = accept(&map, check(&map)?);
    findings.retain(|finding| finding.kind != Kind::AcceptUnused);
    findings.sort_by(|a, b| (a.kind, &a.message).cmp(&(b.kind, &b.message)));
    let problems = findings.len();
    let limit = limit.unwrap_or(100);
    let left_out = problems.saturating_sub(limit);
    findings.truncate(limit);
    let output = Output {
        path: path.clone(),
        placed: run.placed(&map),
        moved: run.moved(&map),
        sides_changed: run.sides_changed(&map),
        notes_placed: run.notes_placed,
        problems,
        findings,
        left_out,
    };
    let output = serde_json::to_value(output).expect("the output is strings and counts");
    if problems == 0 {
        return Ok(output);
    }
    let stranded = run.stranded(&map);
    let hint = if stranded.is_empty() {
        "pin the item a finding names where it should go with `at.set` pinned, then run map.place \
         again; or accept a finding by name with a reason"
            .to_owned()
    } else {
        format!(
            "{} could not be placed at all: give {} room in {} region or lane, or pin {} with \
             `at.set` pinned, then run map.place again",
            names(&stranded),
            if stranded.len() == 1 { "it" } else { "them" },
            if stranded.len() == 1 { "its" } else { "their" },
            if stranded.len() == 1 { "it" } else { "them" },
        )
    };
    let mut error = refusal(
        "map::place::findings_left",
        format!(
            "{path} was placed as well as the engine could, and {problems} findings are left; \
             the best spots are stored"
        ),
        &hint,
        [("path", path.as_str())],
    );
    error.fields.insert("result".to_owned(), output);
    Err(error)
}

/// `ids` joined for a sentence: `a`, `a and b`, or `a, b and c`.
fn names(ids: &[String]) -> String {
    match ids {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// How a run's progress is measured: findings, then the distance from the
/// place a candidate prefers, which breaks ties.
type Score = (usize, f64);

/// Where each item and line note stood before a run, and which the run may
/// choose again.
struct Run {
    /// Each item's id, position and side before the run.
    items: Vec<(String, Option<At>, Option<Side>)>,
    /// The items the run may place or move, in id order.
    free: Vec<String>,
    /// The items with no stored position, in id order.
    unplaced: Vec<String>,
    /// The notes of each line that the run may place again, by index.
    notes: Vec<usize>,
    notes_placed: usize,
}

impl Run {
    fn new(map: &LayoutRecord, relayout: bool) -> Self {
        let mut free: Vec<String> = (map.items.iter())
            .filter(|item| match item.at {
                None => true,
                Some(at) => relayout && at.set == Set::Engine,
            })
            .map(|item| item.id.clone())
            .collect();
        free.sort();
        let mut unplaced: Vec<String> = (map.items.iter())
            .filter(|item| item.at.is_none())
            .map(|item| item.id.clone())
            .collect();
        unplaced.sort();
        let mut notes: Vec<usize> = (map.notes.iter().enumerate())
            .filter(|(_, note)| match &note.anchor {
                Anchor::Line(on) => relayout || on.offset.is_none(),
                Anchor::Item(_) => false,
            })
            .map(|(at, _)| at)
            .collect();
        // The notes are chosen in the order of their text and line, so the
        // order the map lists them in changes nothing.
        notes.sort_by_key(|at| note_key(&map.notes[*at]));
        Self {
            items: (map.items.iter())
                .map(|item| (item.id.clone(), item.at, item.side))
                .collect(),
            free,
            unplaced,
            notes,
            notes_placed: 0,
        }
    }

    /// The items that had no stored position and now have one.
    fn placed(&self, map: &LayoutRecord) -> Vec<String> {
        (self.unplaced.iter())
            .filter(|id| map.items.iter().any(|item| item.id == **id && item.at.is_some()))
            .cloned()
            .collect()
    }

    /// The items that had no stored position and still have none, because
    /// no spot their region, lane and columns allow was left.
    fn stranded(&self, map: &LayoutRecord) -> Vec<String> {
        (self.unplaced.iter())
            .filter(|id| map.items.iter().any(|item| item.id == **id && item.at.is_none()))
            .cloned()
            .collect()
    }

    fn moved(&self, map: &LayoutRecord) -> Vec<String> {
        let mut moved: Vec<String> = (self.items.iter())
            .filter(|(_, at, _)| at.is_some())
            .filter(|(id, at, _)| {
                let now = map
                    .items
                    .iter()
                    .find(|item| item.id == *id)
                    .and_then(|i| i.at);
                now.map(|now| (now.x, now.y)) != at.map(|at| (at.x, at.y))
            })
            .map(|(id, _, _)| id.clone())
            .collect();
        moved.sort();
        moved
    }

    fn sides_changed(&self, map: &LayoutRecord) -> Vec<String> {
        let mut changed: Vec<String> = (self.items.iter())
            .filter(|(id, _, side)| {
                let now = map.items.iter().find(|item| item.id == *id);
                now.map(|item| item.side.unwrap_or_default()) != Some(side.unwrap_or_default())
            })
            .map(|(id, _, _)| id.clone())
            .collect();
        changed.sort();
        changed
    }

    /// Places every free item, refines them, and chooses sides and line
    /// notes again. When findings are left, it searches on by annealing.
    fn place(&mut self, map: &mut LayoutRecord) {
        let ids = self.free.clone();
        // A position chosen again starts from nothing, so it does not
        // depend on where the item stood.
        for id in &ids {
            if let Some(item) = find(map, id) {
                item.at = None;
            }
        }
        for id in &ids {
            Self::choose(map, id, Reach::Whole);
        }
        // The engine does not store where a person moved a pinned item
        // from, so a pinned item a finding touches counts as moved: the
        // sides near it are chosen again, as they are near a placed item.
        let mut moved = ids.clone();
        moved.extend(touched(map));
        moved.sort();
        moved.dedup();
        for _ in 0..PASSES {
            for id in &ids {
                Self::choose(map, id, Reach::Near);
            }
            Self::sides(map, &moved);
        }
        self.line_notes(map);
        self.notes_placed = self.notes.len();
        if problems(map) > 0 {
            self.anneal(map, &ids, &moved);
        }
    }

    /// Moves one random free item or line note at a time, keeping a move
    /// that adds findings with a chance that falls as the search goes, and
    /// ends with the best map seen. The random numbers come from a fixed
    /// seed and the sorted ids, so the same map gives the same answer.
    fn anneal(&self, map: &mut LayoutRecord, ids: &[String], moved: &[String]) {
        let notes = self.notes.clone();
        // A held item's side is chosen again only within one label width
        // of a moved item, as `sides` chooses them.
        let all = neighbours(map, moved);
        if all.is_empty() && notes.is_empty() {
            return;
        }
        let mut random = Random(SEED);
        let mut now = problems(map);
        let mut best = (now, map.items.clone(), map.notes.clone());
        for step in 0..ANNEALING {
            if step > 0 && step % COOLING == 0 {
                map.items.clone_from(&best.1);
                map.notes.clone_from(&best.2);
                now = best.0;
            }
            let heat = 1.0 - crate::geom::count(step % COOLING) / crate::geom::count(COOLING);
            let temperature = 0.05 + 0.7 * heat;
            let mut undo: Option<Undo> = None;
            if !notes.is_empty() && (all.is_empty() || random.below(10) == 0) {
                let at = notes[random.below(notes.len())];
                if let Anchor::Line(on) = &mut map.notes[at].anchor {
                    undo = Some(Undo::Note(at, on.along, on.offset));
                    on.along = ALONGS[random.below(ALONGS.len())];
                    on.offset = Some(OFFSETS[random.below(OFFSETS.len())]);
                }
            } else if let Some(from) = {
                let id = &all[random.below(all.len())];
                map.items.iter().position(|item| item.id == *id)
            } {
                undo = Some(Undo::Item(from, map.items[from].at, map.items[from].side));
                if ids.contains(&map.items[from].id) {
                    let reach = if random.below(3) == 0 {
                        Reach::Whole
                    } else {
                        Reach::Near
                    };
                    let spots = spots(map, from, reach);
                    if !spots.is_empty() {
                        let (x, y) = spots[random.below(spots.len())];
                        set(&mut map.items[from], x, y, SIDES[random.below(SIDES.len())]);
                    }
                } else {
                    // A held item keeps its position and may change its side.
                    map.items[from].side = Some(SIDES[random.below(SIDES.len())]);
                }
            }
            let Some(undo) = undo else { continue };
            let next = problems(map);
            let drop = crate::geom::count(next.saturating_sub(now));
            // A held item keeps a new side only when it clears a finding.
            let keep = match undo {
                Undo::Item(from, ..) if !ids.contains(&map.items[from].id) => next < now,
                _ => next <= now || random.unit() < (-drop / temperature).exp(),
            };
            if keep {
                now = next;
                if now < best.0 {
                    best = (now, map.items.clone(), map.notes.clone());
                    if now == 0 {
                        break;
                    }
                }
            } else {
                match undo {
                    Undo::Item(from, at, side) => {
                        map.items[from].at = at;
                        map.items[from].side = side;
                    }
                    Undo::Note(at, along, offset) => {
                        if let Anchor::Line(on) = &mut map.notes[at].anchor {
                            on.along = along;
                            on.offset = offset;
                        }
                    }
                }
            }
        }
        map.items = best.1;
        map.notes = best.2;
    }

    /// Chooses the best position and side for `id`.
    fn choose(map: &mut LayoutRecord, id: &str, reach: Reach) {
        let Some(from) = map.items.iter().position(|item| item.id == id) else {
            return;
        };
        let spots = spots(map, from, reach);
        let home = home(map, from);
        let (old_at, old_side) = (map.items[from].at, map.items[from].side);
        let mut best: Option<(Score, f64, f64, Side)> = None;
        for &(x, y) in &spots {
            for side in SIDES {
                set(&mut map.items[from], x, y, side);
                let score = (problems(map), (x - home.0).hypot(y - home.1) / 1000.0);
                if best.is_none_or(|(held, ..)| better(score, held)) {
                    best = Some((score, x, y, side));
                }
            }
        }
        if let Some((_, x, y, side)) = best {
            set(&mut map.items[from], x, y, side);
        } else {
            map.items[from].at = old_at;
            map.items[from].side = old_side;
        }
    }

    /// Chooses a side again for each of `ids`, and for each item whose
    /// label is within one label width of one of them.
    fn sides(map: &mut LayoutRecord, ids: &[String]) {
        let near = neighbours(map, ids);
        for id in near {
            let Some(item) = find(map, &id) else {
                continue;
            };
            if item.at.is_none() {
                continue;
            }
            let held = item.side;
            let mut best = (problems(map), held);
            for side in SIDES {
                if let Some(item) = find(map, &id) {
                    item.side = Some(side);
                }
                let score = problems(map);
                if score < best.0 {
                    best = (score, Some(side));
                }
            }
            if let Some(item) = find(map, &id) {
                item.side = best.1;
            }
        }
    }

    /// Places each line note the run may choose again.
    fn line_notes(&self, map: &mut LayoutRecord) {
        let notes = self.notes.clone();
        for at in notes {
            let Anchor::Line(on) = &map.notes[at].anchor else {
                continue;
            };
            let first = on.along;
            let mut best: Option<(usize, f64, f64)> = None;
            let alongs = std::iter::once(first).chain(ALONGS);
            for along in alongs {
                for offset in OFFSETS {
                    if let Anchor::Line(on) = &mut map.notes[at].anchor {
                        on.along = along;
                        on.offset = Some(offset);
                    }
                    let score = problems(map);
                    if best.is_none_or(|(held, ..)| score < held) {
                        best = Some((score, along, offset));
                    }
                }
            }
            if let (Some((_, along, offset)), Anchor::Line(on)) = (best, &mut map.notes[at].anchor)
            {
                on.along = along;
                on.offset = Some(offset);
            }
        }
    }
}

/// How far a search for a spot reaches.
#[derive(Clone, Copy)]
enum Reach {
    /// Every grid position the item's region allows.
    Whole,
    /// Positions within `REFINE` of where the item stands.
    Near,
}

fn better(score: Score, held: Score) -> bool {
    score.0 < held.0 || (score.0 == held.0 && score.1 < held.1 - 1e-9)
}

fn find<'a>(map: &'a mut LayoutRecord, id: &str) -> Option<&'a mut Item> {
    map.items.iter_mut().find(|item| item.id == id)
}

fn set(item: &mut Item, x: f64, y: f64, side: Side) {
    item.at = Some(At {
        x,
        y,
        set: Set::Engine,
    });
    item.side = Some(side);
}

/// How many findings `map` has apart from the ones it accepts and its
/// unplaced items, which a run is about to place.
fn problems(map: &LayoutRecord) -> usize {
    let Ok(found) = check(map) else {
        return usize::MAX;
    };
    let (kept, _) = accept(map, found);
    (kept.iter())
        .filter(|finding| !matches!(finding.kind, Kind::AcceptUnused | Kind::ItemUnplaced))
        .count()
}

/// The place `from` would prefer: where it stands, or the middle of the
/// placed items its lines reach, or its region's middle.
fn home(map: &LayoutRecord, from: usize) -> (f64, f64) {
    let item = &map.items[from];
    if let Some(at) = item.at {
        return (at.x, at.y);
    }
    let reached: Vec<(f64, f64)> = (map.lines.iter())
        .filter_map(|line| {
            let other = if line.from == item.id {
                &line.to
            } else if line.to == item.id {
                &line.from
            } else {
                return None;
            };
            let other = map.items.iter().find(|candidate| candidate.id == *other)?;
            other.at.map(|at| (at.x, at.y))
        })
        .collect();
    let (area, _) = bounds(map, item);
    let middle = (f64::midpoint(area.0, area.2), f64::midpoint(area.1, area.3));
    if reached.is_empty() {
        return middle;
    }
    let n = crate::geom::count(reached.len());
    let (sx, sy) = reached
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
    (
        (sx / n).clamp(area.0, area.2),
        (sy / n).clamp(area.1, area.3),
    )
}

/// The rectangle a mark of `item` may be centred in, as (x0, y0, x1, y1),
/// and whether it is the item's region.
fn bounds(map: &LayoutRecord, item: &Item) -> ((f64, f64, f64, f64), bool) {
    let r = item.radius.unwrap_or(RADIUS);
    if let Some(region) = item
        .band()
        .and_then(|id| map.regions.iter().find(|region| region.id == id))
    {
        return (
            (
                region.x + SIDE_GAP.max(r + 8.0),
                region.y + TOP_GAP,
                region.x + region.width - SIDE_GAP.max(r + 8.0),
                region.y + region.height - BOTTOM_GAP,
            ),
            true,
        );
    }
    (
        (
            SIDE_GAP.max(r + 8.0),
            TOP_GAP,
            map.sheet.width - SIDE_GAP.max(r + 8.0),
            map.sheet.height - BOTTOM_GAP,
        ),
        false,
    )
}

/// The grid positions to try for the item at `from`, in order: down each
/// column from left to right.
fn spots(map: &LayoutRecord, from: usize, reach: Reach) -> Vec<(f64, f64)> {
    let item = &map.items[from];
    let (area, _) = bounds(map, item);
    let (mut x0, mut y0, mut x1, mut y1) = area;
    if let (Reach::Near, Some(at)) = (reach, item.at) {
        x0 = x0.max(at.x - REFINE);
        x1 = x1.min(at.x + REFINE);
        y0 = y0.max(at.y - REFINE);
        y1 = y1.min(at.y + REFINE);
    }
    let mut spots = Vec::new();
    let mut x = (x0 / STEP).ceil() * STEP;
    while x <= x1 {
        let mut y = (y0 / STEP).ceil() * STEP;
        while y <= y1 {
            spots.push((x, y));
            y += STEP;
        }
        x += STEP;
    }
    if let Some(at) = item.at
        && !spots.contains(&(at.x, at.y))
    {
        spots.insert(0, (at.x, at.y));
    }
    let (after, before) = column_limits(map, item);
    spots.retain(|&(x, _)| after < x && x < before);
    spots
}

/// The x a laned item must stay right of and left of, to keep its lane's
/// column order: the largest x of a placed item in a lower column of the
/// same lane, and the smallest x of a placed item in a higher one. An item
/// with no lane or no column has no limit.
fn column_limits(map: &LayoutRecord, item: &Item) -> (f64, f64) {
    let (Some(lane), Some(column)) = (item.lane.as_deref(), item.column) else {
        return (f64::NEG_INFINITY, f64::INFINITY);
    };
    let mut limits = (f64::NEG_INFINITY, f64::INFINITY);
    for other in &map.items {
        let (Some(at), Some(there)) = (other.at, other.column) else {
            continue;
        };
        if other.id == item.id || other.lane.as_deref() != Some(lane) {
            continue;
        }
        if there < column {
            limits.0 = limits.0.max(at.x);
        } else if there > column {
            limits.1 = limits.1.min(at.x);
        }
    }
    limits
}

/// The pinned items a finding touches: those whose removal from the map
/// clears at least one finding, in id order.
fn touched(map: &mut LayoutRecord) -> Vec<String> {
    let now = problems(map);
    if now == 0 {
        return Vec::new();
    }
    let mut touched = Vec::new();
    for at in 0..map.items.len() {
        let Some(held) = map.items[at].at.filter(|at| at.set == Set::Pinned) else {
            continue;
        };
        map.items[at].at = None;
        if problems(map) < now {
            touched.push(map.items[at].id.clone());
        }
        map.items[at].at = Some(held);
    }
    touched.sort();
    touched
}

/// `ids` and the items whose labels lie within one label width of one of
/// them, in id order.
fn neighbours(map: &LayoutRecord, ids: &[String]) -> Vec<String> {
    let width = |item: &Item| {
        let mark = Mark {
            x: 0.0,
            y: 0.0,
            r: item.radius.unwrap_or(RADIUS),
        };
        let area = label_box(&mark, Side::Right, &item.label);
        area.x1 - area.x0
    };
    let moved: Vec<&Item> = map.items.iter().filter(|i| ids.contains(&i.id)).collect();
    let mut near: Vec<String> = (map.items.iter())
        .filter(|item| {
            let reach = width(item);
            moved.iter().any(|other| {
                item.id == other.id
                    || match (item.at, other.at) {
                        (Some(a), Some(b)) => {
                            (a.x - b.x).hypot(a.y - b.y) <= reach.max(width(other))
                        }
                        _ => false,
                    }
            })
        })
        .map(|item| item.id.clone())
        .collect();
    near.sort();
    near
}

/// What a step of annealing changed, to put back when it is not kept.
enum Undo {
    Item(usize, Option<At>, Option<Side>),
    Note(usize, f64, Option<f64>),
}

/// A xorshift generator: the same seed gives the same numbers.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number from 0 to `n`, not including `n`.
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap_or(1)).unwrap_or(0)
    }

    /// A number from 0 to 1.
    fn unit(&mut self) -> f64 {
        f64::from(u32::try_from(self.next() >> 40).unwrap_or(0)) / f64::from(1u32 << 24)
    }
}

/// What orders a note among the others: its text, then its line.
fn note_key(note: &crate::records::Note) -> (String, String, String) {
    match &note.anchor {
        Anchor::Line(on) => (note.text.clone(), on.from.clone(), on.to.clone()),
        Anchor::Item(on) => (note.text.clone(), on.id.clone(), String::new()),
    }
}

#[cfg(test)]
mod tests;
