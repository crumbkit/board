/** A pending effect, as `board/effect/v1` at `/effects/pending/<id>` in the personal board holds it. */
export type Effect = {
  id: string
  /** The root directory of the board the effect targets. */
  board: string
  plugin: string
  op: string
  path: string
  /** The record or text a put would write, as text; null for an op with no content. */
  content: string | null
  /** Whether a put replaces a file already there. */
  replaces: boolean
}

/** The effects no seen record answers, and whether Board listed only the first of them. */
export type Pending = { effects: Effect[]; truncated: boolean }

declare module 'claude-code' {
  interface PluginState {
    'board-effects': {
      /** The pending effects; null until the first read, or when it failed. */
      pending: Pending | null
    }
  }
}
