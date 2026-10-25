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
  blocks: string | null
}

/** What `plans.open` answered: the questions it listed and how many are open in all. */
export type Open = { questions: Question[]; total: number }

declare module 'claude-code' {
  interface PluginState {
    'board-plans': {
      /** The open questions; null until the first read, or when it failed. */
      open: Open | null
      /** The last morning report, as Markdown; null before /yesterday ran. */
      report: string | null
    }
  }
}
