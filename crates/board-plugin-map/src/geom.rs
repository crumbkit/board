//! Where a reader sees each part of a map: label and note boxes, line ends
//! and anchored note positions, computed from stored positions. Ported from
//! the map prototype's `geom.js`, with the same constants, so its findings
//! match the prototype's checker. Text width is estimated from the number
//! of characters, since Board has no font.

use crate::records::Side;

/// The estimated width of one character of a label's first line.
const NAME_WIDTH: f64 = 7.4;
/// The estimated width of one character of a label's later lines.
const DETAIL_WIDTH: f64 = 6.5;
/// The estimated width of one character of a note.
const NOTE_WIDTH: f64 = 6.6;
/// The estimated width of one character of a region's title.
const TITLE_WIDTH: f64 = 7.6;
/// The height of one line of text.
const LINE_HEIGHT: f64 = 15.0;
/// The radius of a mark whose item gives none.
pub(crate) const RADIUS: f64 = 8.0;
/// The offset across its line of a line note that gives none.
pub(crate) const OFFSET: f64 = 9.0;

/// An axis-aligned rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Box2 {
    pub(crate) x0: f64,
    pub(crate) y0: f64,
    pub(crate) x1: f64,
    pub(crate) y1: f64,
}

impl Box2 {
    /// Whether the two boxes share any area.
    pub(crate) fn hits(&self, other: &Self) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }

    /// Whether the box lies wholly inside `outer`.
    pub(crate) fn within(&self, outer: &Self) -> bool {
        self.x0 >= outer.x0 && self.y0 >= outer.y0 && self.x1 <= outer.x1 && self.y1 <= outer.y1
    }

    /// The point of the box nearest (x, y).
    fn nearest(&self, x: f64, y: f64) -> (f64, f64) {
        (x.clamp(self.x0, self.x1), y.clamp(self.y0, self.y1))
    }

    /// The distance from the box to (x, y), 0 inside it.
    pub(crate) fn distance(&self, x: f64, y: f64) -> f64 {
        let (nx, ny) = self.nearest(x, y);
        (x - nx).hypot(y - ny)
    }

    /// The box's centre.
    pub(crate) fn centre(&self) -> (f64, f64) {
        (f64::midpoint(self.x0, self.x1), f64::midpoint(self.y0, self.y1))
    }
}

/// A placed mark: its centre and radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) r: f64,
}

impl Mark {
    /// The square around the mark.
    pub(crate) fn square(&self) -> Box2 {
        Box2 {
            x0: self.x - self.r,
            y0: self.y - self.r,
            x1: self.x + self.r,
            y1: self.y + self.r,
        }
    }
}

/// `n` as a float; a map never holds more than `u32::MAX` of anything.
pub(crate) fn count(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Rounds as JavaScript's `Math.round` does: halves go up.
fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// The box a label of `lines` takes on `side` of `mark`.
pub(crate) fn label_box(mark: &Mark, side: Side, lines: &[String]) -> Box2 {
    let r = mark.r + 1.0;
    let width = (lines.iter().enumerate())
        .map(|(index, line)| {
            let each = if index == 0 { NAME_WIDTH } else { DETAIL_WIDTH };
            count(line.chars().count()) * each
        })
        .fold(f64::NEG_INFINITY, f64::max);
    let more = count(lines.len().saturating_sub(1));
    let height = count(lines.len()) * LINE_HEIGHT;
    let (x0, y) = match side {
        Side::Right => (mark.x + r + 6.0, mark.y + 5.0 - more * 7.5),
        Side::Left => (mark.x - r - 6.0 - width, mark.y + 5.0 - more * 7.5),
        Side::Below => (mark.x - width / 2.0, mark.y + r + 15.0),
        Side::Above => (mark.x - width / 2.0, mark.y - r - 10.0 - more * LINE_HEIGHT),
    };
    Box2 {
        x0,
        y0: y - 11.0,
        x1: x0 + width,
        y1: y - 11.0 + height,
    }
}

/// How a note's text is aligned on its point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Align {
    Start,
    Middle,
    End,
}

/// A note's point: where its text is aligned and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NotePoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) align: Align,
}

/// The point of a note on the line from `a` to `b`, `along` of the way and
/// `offset` across it, on the side of the line to the right on the sheet.
pub(crate) fn line_note(a: &Mark, b: &Mark, along: f64, offset: f64) -> NotePoint {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = nonzero(dx.hypot(dy));
    let (mut nx, mut ny) = (-dy / length, dx / length);
    if nx < 0.0 {
        (nx, ny) = (-nx, -ny);
    }
    let across = nx * offset;
    let align = if across > 2.0 {
        Align::Start
    } else if across < -2.0 {
        Align::End
    } else {
        Align::Middle
    };
    NotePoint {
        x: round(a.x + dx * along + nx * offset),
        y: round(a.y + dy * along + ny * offset + 4.0),
        align,
    }
}

/// The box a note of `text` takes at `point`.
pub(crate) fn note_box(point: &NotePoint, text: &str) -> Box2 {
    let width = count(text.chars().count()) * NOTE_WIDTH;
    let x0 = match point.align {
        Align::Start => point.x,
        Align::Middle => point.x - width / 2.0,
        Align::End => point.x - width,
    };
    Box2 {
        x0,
        y0: point.y - 11.0,
        x1: x0 + width,
        y1: point.y + 3.0,
    }
}

/// The box a region's title takes, from the region's top left corner.
pub(crate) fn title_box(x: f64, y: f64, title: &str) -> Box2 {
    Box2 {
        x0: x + 8.0,
        y0: y + 6.0,
        x1: x + 16.0 + count(title.chars().count()) * TITLE_WIDTH,
        y1: y + 22.0,
    }
}

/// A drawn line segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Segment {
    pub(crate) x1: f64,
    pub(crate) y1: f64,
    pub(crate) x2: f64,
    pub(crate) y2: f64,
}

/// The segment a line from `a` to `b` is drawn as, from the edge of one
/// mark to the edge of the other, with a gap at each end.
pub(crate) fn line_ends(a: &Mark, b: &Mark) -> Segment {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = nonzero(dx.hypot(dy));
    let (ux, uy) = (dx / length, dy / length);
    Segment {
        x1: a.x + ux * (a.r + 2.0),
        y1: a.y + uy * (a.r + 2.0),
        x2: b.x - ux * (b.r + 3.0),
        y2: b.y - uy * (b.r + 3.0),
    }
}

/// `length`, or 1 when it is 0, so a line between two marks at one point
/// has a direction.
fn nonzero(length: f64) -> f64 {
    if length > 0.0 { length } else { 1.0 }
}

impl Segment {
    /// Whether the segment enters `area`.
    pub(crate) fn crosses(&self, area: &Box2) -> bool {
        let inside = |x: f64, y: f64| x > area.x0 && x < area.x1 && y > area.y0 && y < area.y1;
        if inside(self.x1, self.y1) || inside(self.x2, self.y2) {
            return true;
        }
        let Box2 { x0, y0, x1, y1 } = *area;
        [
            (x0, y0, x1, y0),
            (x1, y0, x1, y1),
            (x1, y1, x0, y1),
            (x0, y1, x0, y0),
        ]
        .iter()
        .any(|&(ax, ay, bx, by)| self.cuts(ax, ay, bx, by))
    }

    /// Whether the segment and the one from (ax, ay) to (bx, by) cross at
    /// a point inside both.
    fn cuts(&self, ax: f64, ay: f64, bx: f64, by: f64) -> bool {
        let turn = |px: f64, py: f64, qx: f64, qy: f64, rx: f64, ry: f64| {
            sign((qx - px) * (ry - py) - (qy - py) * (rx - px))
        };
        let Self { x1, y1, x2, y2 } = *self;
        turn(x1, y1, x2, y2, ax, ay) * turn(x1, y1, x2, y2, bx, by) < 0
            && turn(ax, ay, bx, by, x1, y1) * turn(ax, ay, bx, by, x2, y2) < 0
    }

    /// The distance from (px, py) to the nearest point of the segment.
    pub(crate) fn distance(&self, px: f64, py: f64) -> f64 {
        let (dx, dy) = (self.x2 - self.x1, self.y2 - self.y1);
        let squared = dx * dx + dy * dy;
        let squared = if squared > 0.0 { squared } else { 1.0 };
        let t = (((px - self.x1) * dx + (py - self.y1) * dy) / squared).clamp(0.0, 1.0);
        (px - (self.x1 + t * dx)).hypot(py - (self.y1 + t * dy))
    }
}

/// The sign of `value`: -1, 0 or 1.
fn sign(value: f64) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}
