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

/** What `plans.waiting` counts: everything that waits on the owner, and how much of it blocks work. */
export type Waiting = { total: number; blocking: number }

declare module 'claude-code' {
  interface PluginState {
    'board-plans': {
      /** The open questions; null until the first read, or when it failed. */
      open: Open | null
      /** What waits on the owner; null until plans.waiting answers, and while it does not exist. */
      waiting: Waiting | null
      /** The last morning report, as Markdown; null before /yesterday ran. */
      report: string | null
    }
  }
}
