export const task = {
  $type: "tasks/task/v1",
  title: "Document the retry limit",
  status: "open",
  estimate: 2,
  done: false,
};

export const nested = {
  $type: "review/review/v1",
  title: "Sync change",
  page: "/bindings/sync-review",
  author: { agent: "claude-code", model: "claude-opus-5-5" },
  files: ["crates/board-ops/src/sync.rs", "crates/board-ops/src/sync/tests.rs"],
  verdict: null,
};
