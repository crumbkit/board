/** A rule's kind: a standing rule, an obligation owed, or a measured budget. */
export type Kind = 'standing' | 'obligation' | 'budget'

/** A rule that `rules.check` or `rules.for-path` named, with what it asks. */
export type Rule = {
  kind: Kind
  rule: string
  requirement: string
  /** For a measured rule, its usage, such as `412 of 1000 words`. */
  usage?: string
  /** True for a rule with no checker, which only a reader judges. */
  described?: boolean
  /** For an obligation, how to close it, as the rules plugin gives it. */
  close?: string
}

/** What `board call rules.check` answered for the repository a session starts in. */
export type Checked =
  | { kind: 'read'; root: string; rules: Rule[] }
  | { kind: 'failed'; root: string; code: string; message: string }

/**
 * A read-only badge of the session's state, such as `Board personal`, or a
 * red badge that says what is wrong with the badges file.
 */
export type StateBadge = { label: string; value: string; problem: null } | { problem: string }

declare module 'claude-code' {
  interface PluginState {
    'board-rules': {
      /** The rules on the session's repository; null when no board holds it. */
      checked: Checked | null
      /** The rules each conversation was told about, keyed by agent id, '' for the main one. */
      told: Record<string, string[]>
      /** The state badges read at session start. */
      state: StateBadge[]
    }
  }
}
