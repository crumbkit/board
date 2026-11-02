/** The task each conversation claimed with tasks.claim, by agent id, '' for the main one. */
export type Claimed = Record<string, string>

declare module 'claude-code' {
  interface PluginState {
    'board-artifacts': {
      /** The task each conversation claimed with tasks.claim. */
      claimed: Claimed
    }
  }
}
