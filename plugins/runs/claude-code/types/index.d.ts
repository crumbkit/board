/** When each delegated unit that has not stopped yet started, in milliseconds, by `agent:<id>` or `task:<id>`. */
export type Started = Record<string, number>
/** The agent record the mod wrote for each subagent that has not stopped yet, by agent id. */
export type Agents = Record<string, string>

declare module 'claude-code' {
  interface PluginState {
    'board-runs': {
      started: Started
      agents: Agents
      /** How many records the mod wrote this session. */
      written: number
    }
  }
}
