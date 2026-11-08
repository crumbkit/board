/** An order an agent asked the owner to sign: the record's path in the personal board. */
export type Request = { path: string }

/** A pinned order that `attention.pinned` verified. */
export type Order = {
  path: string
  signer: string
  order: string
  expires: string
  context: string
  context_text: string
}

/** A pinned order `attention.pinned` left out, with the code that says why. */
export type LeftOut = { path: string; code: string; message: string }

/**
 * What `board call attention.pinned` answered at session start, or why it
 * could not be read. `none` means there is no personal board, so nothing is
 * pinned and the mod shows nothing.
 */
export type Pinned =
  | { kind: 'read'; board: string; orders: Order[]; leftOut: LeftOut[] }
  | { kind: 'failed'; board: string; code: string; message: string }
  | { kind: 'none' }

declare module 'claude-code' {
  interface PluginState {
    'board-attention': {
      /** Requests waiting for the owner, oldest first. */
      pending: Request[]
      /** The path whose signing window is open now, if any. */
      open: string | null
      /** The pinned orders read at session start; null until they are read. */
      pinned: Pinned | null
      /** The order the pane shows alone, chosen by its badge; null shows every order. */
      selected: string | null
      /** Whether the pane shows the chosen order's context. */
      showContext: boolean
    }
  }
}
