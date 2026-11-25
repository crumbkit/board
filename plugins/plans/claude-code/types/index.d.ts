/** One answer the owner may give to a question. */
export type Option = { id: string; text: string }

/** An open question, as `board call plans.open` lists it. */
export type Question = {
  path: string
  question: string
  asked: string
  /** The session that asked; absent when a person asked without one. */
  session?: string
  /** The person who asked, when the asker named one. */
  person?: string
  /** The other sessions that asked the same question, each of which waits for its answer too. */
  also_asked?: string[]
  options: Option[]
  recommendation: string | null
  /** The change the question holds up, by its path. */
  blocks: string | null
  /** What the question holds up, in words. */
  blocked_work: string | null
}

/** What `plans.open` answered: the questions it listed and how many are open in all. */
export type Open = { questions: Question[]; total: number }

/** Why the last read of each count failed, as Board's code and message; null when it worked or has not run. */
export type Unread = { open: string | null; waiting: string | null }

/** What `plans.waiting` counts: the open questions, and how many of them block work. */
export type Waiting = { total: number; blocking: number }

/** The kind of check a ready change waits on, as `changes.ready` names it. */
export type ReadyKind = 'recheck' | 'data-sign-off' | 'deviation' | 'land-choice'

/** Why a ready change is failing, as `changes.ready` names it. */
export type Failing = 'ci_failed' | 'conflict' | 'stale'

/** A change ready to land that waits only on the owner, as `changes.ready` lists it. */
export type ReadyChange = {
  path: string
  title: string
  /** The change's newest pull request record, by its path. */
  pull: string | null
  head: string | null
  kind: ReadyKind
  /** The missing check it waits on; null when only a question holds it. */
  check: string | null
  /** What that check or question asks. */
  waits_on: string
  /** The open questions whose `blocks` names the change. */
  questions: string[]
  /** The days since the change was last touched. */
  days: number | null
  failing: Failing | null
  /** Whether the change is done and owes only a recheck; absent from a board older than this field. */
  done?: boolean
}

/** What `changes.ready` answered: the changes it listed and how many there are in all. */
export type Ready = { changes: ReadyChange[]; total: number }

declare module 'claude-code' {
  interface PluginState {
    'board-plans': {
      /** The open questions; null until the first read, or when it failed. */
      open: Open | null
      /** The open questions plans.waiting counts; null until it answers, or when it failed. */
      waiting: Waiting | null
      /** Why the last read of the counts failed, so the band can say the count is stale or missing. */
      failed: Unread
      /** The last morning report, as Markdown; null before /yesterday ran. */
      report: string | null
      /** The changes ready to land; null until changes.ready answers. */
      ready: Ready | null
      /** Why the last read of changes.ready failed; null when it worked or has not run. */
      readyFailed: string | null
    }
  }
}
