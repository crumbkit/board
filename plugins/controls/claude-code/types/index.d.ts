/** A control that `controls.check` or `controls.for-path` named, with what it asks. */
export type Control = {
  control: string
  requirement: string
  /** For a measured control, its usage, such as `412 of 1000 words`. */
  usage?: string
}

/** What `board call controls.check` answered for the repository a session starts in. */
export type Checked =
  | { kind: 'read'; root: string; controls: Control[] }
  | { kind: 'failed'; root: string; code: string; message: string }

declare module 'claude-code' {
  interface PluginState {
    'board-controls': {
      /** The controls on the session's repository; null when no board holds it. */
      checked: Checked | null
      /** The controls each conversation was told about, keyed by agent id, '' for the main one. */
      told: Record<string, string[]>
    }
  }
}
