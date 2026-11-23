// When the mod reads the count of what waits and the open list again.
//
// Each read runs `board`, which takes a snapshot of the whole board and costs
// about a second. So the mod reads only for a session someone may be looking
// at: the minute's tick reads the count only when the band was drawn since
// the last read, and a draw reads at once when the last read is more than a
// minute old. The open list, which the pane shows, is read only when the
// pane is drawn and the list is more than a minute old, when the count of
// what waits has changed since the list was read, or when there is no count
// and the button counts the list.
//
// The functions here decide, and a function that decides to read marks the
// read started at once, because the host may draw twice before the read
// runs. register.tsx runs the reads and marks each list read ended.

/** How long a read of the count, or of the open list, stays fresh. */
export const REFRESH_MS = 60_000

/** What the mod knows about its last reads, kept in memory for the session. */
export type Schedule = {
  /** When the last read of the count started, in ms; null before the first. */
  read: number | null
  /** Whether the band was drawn since the last read of the count started. */
  drawn: boolean
  /** Whether a read of the count is running. */
  reading: boolean
  /** When the last read of the open list started, in ms; null before the first. */
  listed: number | null
  /** Whether a read of the open list is running. */
  listing: boolean
  /** What plans.waiting counted when the open list was last read; null when it did not count. */
  listedTotal: number | null
}

/** How the read of plans.waiting ended: a count, no such capability, or a failure. */
export type Counted = { kind: 'read'; total: number } | { kind: 'absent' } | { kind: 'failed' }

export const schedule = (): Schedule => ({ read: null, drawn: false, reading: false, listed: null, listing: false, listedTotal: null })

const stale = (at: number | null, now: number) => at === null || now - at >= REFRESH_MS

/** Whether the minute's tick at `now` reads the count: only when the band was drawn since the last read. */
export function tickReads(s: Schedule, now: number): boolean {
  if (!s.drawn || s.reading) return false
  startRead(s, now)
  return true
}

/** Marks the band drawn at `now`, and says whether the draw reads the count at once because the last read is more than a minute old. */
export function drawReads(s: Schedule, now: number): boolean {
  s.drawn = true
  if (s.reading || !stale(s.read, now)) return false
  startRead(s, now)
  return true
}

/** Whether a draw of the questions pane at `now` reads the open list. */
export function paneLists(s: Schedule, now: number): boolean {
  if (s.listing || !stale(s.listed, now)) return false
  startList(s, now)
  return true
}

/**
 * Whether a read of the count that ended as `counted` reads the open list
 * too: when there is no count, so the button counts the list, or when the
 * count differs from the one the list was read beside. A failed count keeps
 * the last list, and the next read retries.
 */
export function countLists(s: Schedule, counted: Counted, now: number): boolean {
  if (s.listing) return false
  if (counted.kind !== 'absent' && (counted.kind !== 'read' || counted.total === s.listedTotal)) return false
  startList(s, now)
  return true
}

/** Marks a read of the count started at `now`, as a session starts or after an answer. */
export function startRead(s: Schedule, now: number) {
  s.read = now
  s.drawn = false
  s.reading = true
}

/** Marks a read of the open list started at `now`. */
export function startList(s: Schedule, now: number) {
  s.listed = now
  s.listing = true
}

/** Marks a read of the count ended. */
export function endRead(s: Schedule) {
  s.reading = false
}

/** Marks a read of the open list ended, beside the count `total` when it worked; a failed read leaves the total, so a changed count retries it. */
export function endList(s: Schedule, worked: boolean, total: number | null) {
  s.listing = false
  if (worked) s.listedTotal = total
}
