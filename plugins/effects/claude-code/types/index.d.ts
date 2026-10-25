/** A pending effect, as the mod's query reads `board/effect/v1` at `/effects/pending/<id>` in the personal board. */
export type Effect = {
  id: string
  /** The root directory of the board the effect targets, with the home directory written as `~`. */
  board: string
  plugin: string
  op: string
  path: string
  /** Whether a put replaces a file already there. */
  replaces: boolean
  /** The first 2,000 characters of the record or text a put would write; null for an op with no content. */
  head: string | null
  /** The size in bytes and the number of lines of the whole content. */
  bytes: number
  lines: number
}

/** The pending effects of the last read that worked, and why the latest read failed, if it did. */
export type Pending = {
  effects: Effect[]
  /** Whether Board listed only the first of them. */
  truncated: boolean
  /** Why the latest read failed; null when it worked. */
  failed: string | null
}

declare module 'claude-code' {
  interface PluginState {
    'board-effects': {
      /** The pending effects; null until the first read. */
      pending: Pending | null
    }
  }
}
