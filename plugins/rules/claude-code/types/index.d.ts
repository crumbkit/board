/** A rule's kind: a standing rule, an obligation owed, or a measured budget. */
export type Kind = 'standing' | 'obligation' | 'budget'

/** A rule that `rules.check` or `rules.for-path` named, with what it asks. */
export type Rule = {
  kind: Kind
  rule: string
  requirement: string
  /** For a measured rule, its usage, such as `412 of 1000 words` or `remaining 0 in /suites/s/runs/1, 20 h old`. */
  usage?: string
  /** True for a rule with no checker, which only a reader judges. */
  described?: boolean
  /** For an obligation, how to close it, as the rules plugin gives it. */
  close?: string
}

/**
 * What `board call rules.check` answered for the repository a session starts
 * in. A read answer with `at` is one the mod remembered from an earlier
 * check, at that time in milliseconds; `pending` is a first check still
 * running.
 */
/** Something `rules.check` says of a rule without failing it, such as `board::ref::unreadable`. */
export type Warning = { code: string; detail: string }

export type Checked =
  | { kind: 'read'; root: string; rules: Rule[]; warnings?: Warning[]; at?: number; stale?: { code: string; message: string } }
  | { kind: 'failed'; root: string; code: string; message: string }
  | { kind: 'pending'; root: string }

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
      /** Whether the main conversation's first message has carried the block. */
      sent: boolean
    }
  }
}
