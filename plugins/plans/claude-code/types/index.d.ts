/** One answer the owner may give to a question. */
export type Option = { id: string; text: string }

/** An open question, as `board call plans.open` lists it. */
export type Question = {
  path: string
  question: string
  asked: string
  session: string
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
    }
  }
}
