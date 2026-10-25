export const short = {
  markdown: "The change moves the retry loop into `sync` and adds a test for a remote that refuses twice.",
};

export const long = {
  markdown: `# Review of the sync change

The change moves the retry loop into \`sync\`, so every caller gets the same
limit. It adds one test.[^1]

## What changed

- [x] The loop lives in one place
- [x] A remote that refuses twice is retried
- [ ] The limit is documented

\`\`\`rust
fn sync(board: &Board) -> Result<Report, SyncError> {
    retry(3, || board.push())
}
\`\`\`

## Risks

| Case | Before | After |
| --- | --- | --- |
| remote refuses once | fails | retried |
| remote refuses four times | fails | fails |

### Open questions

Should the limit be a flag?

[^1]: \`a_remote_that_refuses_twice_is_retried\` in \`writes/tests.rs\`.
`,
};
