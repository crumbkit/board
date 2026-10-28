/** A pending effect, as the mod's query reads `board/effect/v1` at `/effects/pending/<id>` in the personal board. */
export type Effect = {
  id: string
  /** The root directory of the board the effect targets, with the home directory written as `~`. */
  board: string
  plugin: string
  op: string
  path: string
}

/** The pending effects of the last read that worked, and why the latest read failed, if it did. */
export type Pending = {
  effects: Effect[]
  /** Whether Board listed only the first of them. */
  truncated: boolean
  /** Why the latest read failed; null when it worked. */
  failed: string | null
  /** Whether any read has worked, so `effects` is a list Board gave. */
  worked: boolean
}

declare module 'claude-code' {
  interface PluginState {
    'board-effects': {
      /** The pending effects; null until the first read. */
      pending: Pending | null
    }
  }
}
