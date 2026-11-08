/** A control's kind: a standing rule, an obligation owed, or a measured budget. */
export type Kind = 'standing' | 'obligation' | 'budget'

/** A control that `controls.check` or `controls.for-path` named, with what it asks. */
export type Control = {
  kind: Kind
  control: string
  requirement: string
  /** For a measured control, its usage, such as `412 of 1000 words`. */
  usage?: string
  /** For an obligation, how to close it, as the controls plugin gives it. */
  close?: string
}

/** What `board call controls.check` answered for the repository a session starts in. */
export type Checked =
  | { kind: 'read'; root: string; controls: Control[] }
  | { kind: 'failed'; root: string; code: string; message: string }

/**
 * A read-only badge of the session's state, such as `Board personal`, or a
 * red badge that says what is wrong with the badges file.
 */
export type StateBadge = { label: string; value: string; problem: null } | { problem: string }

declare module 'claude-code' {
  interface PluginState {
    'board-controls': {
      /** The controls on the session's repository; null when no board holds it. */
      checked: Checked | null
      /** The controls each conversation was told about, keyed by agent id, '' for the main one. */
      told: Record<string, string[]>
      /** The state badges read at session start. */
      state: StateBadge[]
    }
  }
}
