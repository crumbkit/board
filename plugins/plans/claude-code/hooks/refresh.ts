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
// A read the tick or a draw starts runs `board` only when the board's
// stamp, in stamp.ts, changed since the last read that worked; register.tsx
// still marks the read begun and ended, so the next check is a minute away.
//
// The predicates here only answer. register.tsx calls `begin` as soon as a
// predicate says a read is due, before the read runs, because the host may
// draw twice before a read that a draw schedules starts.

/** How long a read of the count, or of the open list, stays fresh. */
export const REFRESH_MS = 60_000

/**
 * How long a read may run before it counts as lost. Each `board` run stops
 * after 5 seconds, and a count read may run two of them in turn, so a read
 * still marked running after this never ended, such as one whose scheduled
 * callback never ran. A lost read does not stop the next one.
 */
export const LOST_MS = 30_000

/** One kind of read: when the last one began, and when the one in flight began. */
export type Read = { began: number | null; running: number | null }

/** What the mod knows about its reads, kept in memory for the session. */
export type Schedule = {
  count: Read
  list: Read
  /** Whether the band was drawn since the last read of the count began. */
  drawn: boolean
  /** What plans.waiting counted when the open list was last read; null when it did not count. */
  listedTotal: number | null
}

/** How the read of plans.waiting ended: a count, no such capability, or a failure. */
export type Counted = { kind: 'read'; total: number } | { kind: 'absent' } | { kind: 'failed' }

export const schedule = (): Schedule => ({ count: { began: null, running: null }, list: { began: null, running: null }, drawn: false, listedTotal: null })

/** Whether a read of this kind is in flight and not yet lost. */
export const isRunning = (r: Read, now: number) => r.running !== null && now - r.running < LOST_MS

/** Whether a read of this kind is due: none is running and the last is more than a minute old. */
export const isDue = (r: Read, now: number) => !isRunning(r, now) && (r.began === null || now - r.began >= REFRESH_MS)

/** Whether the minute's tick reads the count: only when the band was drawn since the last read. */
export const tickIsDue = (s: Schedule, now: number) => s.drawn && !isRunning(s.count, now)

/**
 * Whether a read of the count that ended as `counted` calls for the open list
 * too: when there is no count, so the button counts the list, or when the
 * count differs from the one the list was read beside. A failed count keeps
 * the last list, and the next read retries.
 */
export function countCallsForList(s: Schedule, counted: Counted, now: number): boolean {
  if (isRunning(s.list, now)) return false
  return counted.kind === 'absent' || (counted.kind === 'read' && counted.total !== s.listedTotal)
}

/** Marks a read of this kind begun at `now`; a read of the count also clears `drawn`. */
export function begin(s: Schedule, kind: 'count' | 'list', now: number) {
  s[kind] = { began: now, running: now }
  if (kind === 'count') s.drawn = false
}

/** Marks a read of this kind ended. A list read that worked records the count `total` it was read beside; a failed one leaves the old total, so a changed count retries it. */
export function end(s: Schedule, kind: 'count' | 'list', listed?: { worked: boolean; total: number | null }) {
  s[kind].running = null
  if (listed?.worked) s.listedTotal = listed.total
}
