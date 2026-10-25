/** When each delegated unit that has not stopped yet started, in milliseconds, by `agent:<id>` or `task:<id>`. */
export type Started = Record<string, number>

declare module 'claude-code' {
  interface PluginState {
    'board-runs': {
      started: Started
      /** How many records the mod wrote this session. */
      written: number
    }
  }
}
