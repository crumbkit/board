# board-ui and the effect gate

Agents show records to the owner through a catalogue of UI components that
`board serve` serves, and plugins ask Board before they take an effect. When
Board's answer is ask, the owner answers from the command line or a gate
window. PLAN.md links this leaf; when the milestone lands, its settled design
moves into docs/ and this file is deleted. The owner approved this design on
2026-10-25.

## Build order

Each step is one pull request. Steps 2, 3 and 4 change authority or open a
listener, so they also get an adversarial security review.

1. This leaf and the scope change in PLAN.md.
2. Effects without UI: `board/preference/v1`, board settings, `board decide`,
   the check in `put`, `put --or-propose`, pending effects, `board/seen/v1`
   and `board answer`.
3. `board serve`, `ui/view/v1` and `ui/binding/v1`, the app skeleton with its
   tokens, and the `markdown` and `record` kinds.
4. The `Gate` trait, `WindowGate`, the `gate` kind and Touch ID signing of
   saved rules.
5. The `diff` kind.
6. The `eval`, `run` and `walkthrough` kinds.

## Decided

1. **No answer depends on `board serve`.** Every capability works from the
   command line. The UI shows records and collects answers the CLI can also
   give.
2. **`ui` is core.** `board serve` and `board ui` are in the board binary, so
   `ui` is a reserved namespace beside `board`. The first users of views are
   `review` and `agentic-loop`.
3. **Views and bindings are records.** Every UI an agent puts together is
   recorded, so the compositions agents choose can be studied later.
   - `ui/view/v1` at `/views/<name>.json` holds only the composition: a list
     of named slots, each with a component kind.
   - `ui/binding/v1` at `/bindings/<name>.json` names a view and fills each
     slot with `{ "ref": <record path> }` or `{ "data": … }`.
   - Content derived for a page, such as a model-written summary, is its own
     record. It lists the records it came from and names the agent and model
     that wrote it. The binding cites it by `ref`. Step 3 names its type.
   - `put` checks every slot's data or referenced record against its kind's
     schema and refuses a mismatch with a `board::ui::` code.
4. **Commands.**
   - `board ui kinds` lists the catalogue, with each kind's JSON Schema and the
     record types it draws directly.
   - `board ui template <kind>` prints a filled-in example to edit.
   - `board ui open /bindings/<name>` starts the server if it is not running
     and opens the bound view in the browser.
5. **`board serve`** is Rust in the board binary.
   - It binds 127.0.0.1 on a random port and writes the port to a file in
     the board's cache directory, where `board ui open` finds it. It exits
     after 30 minutes without a request.
   - Every request carries a per-launch token, which `board ui open` puts in
     the URL it opens.
   - It refuses a request whose Host header is not `127.0.0.1:<port>`. This
     stops a web page reaching it through a DNS name that resolves to
     localhost.
   - Routes: `GET /` serves the embedded app; `GET /api/bindings/<name>`
     returns the binding and its view with every `ref` resolved, and diffs
     computed through `board-tree`; `POST /api/gate/<id>` takes the owner's
     answer to a pending effect and is the only write route.
6. **The built app is committed** and embedded with `include_dir!`, so
   viitta builds with only Rust installed. CI fails when the committed bundle
   is stale.
7. **Toolchain.** React 19, Vite, Tailwind 4, shadcn/ui, Shiki, Storybook,
   Vitest, Playwright and pnpm, with stable TypeScript. board-ui has its own
   tokens and look. Goldens: Storybook's Vitest addon with
   `toMatchScreenshot`, one test per story, light and dark projects, traces
   kept on failure, run only in the pinned Playwright container. Step 3
   spikes the addon; the fallback is Playwright `toHaveScreenshot`.
8. **Each kind ships** a JSON Schema, a React component, a Storybook story
   with fixtures, and a golden screenshot in both themes. The first catalogue:
   - `markdown`: GitHub-flavoured Markdown, Shiki code, heading anchors with a
     contents rail, footnotes, task lists, Mermaid, a 68-character measure.
   - `record`: any record as a definition list, nested objects folded, refs as
     links.
   - `diff`: a commit range or patch; unified and split views, word-level
     changes, folded unchanged regions, a file tree with change counts,
     linkable line anchors.
   - `eval`: `evals/eval/v1` with its suites and runs; verdict, each suite's
     method, a pass/fail grid per case from the runs, the key numbers, runs
     over time.
   - `run`: `runs/run/v1`; suite, outcome, duration, failures first with
     excerpts.
   - `walkthrough`: ordered steps, each with action, screenshot (optional
     highlight), observation and outcome. Drawn from a user-journey
     `runs/run/v1` (`note`, `evidence`, linked eval) or steps an agent binds
     inline, citing screenshots by path.
   - `gate`: a pending effect and a drafted rule (decision 11).
9. **Effects, first slice.** A plugin asks Board whether it may take an
   effect on a path, and Board answers act, propose or ask. The first users
   are `review`, which posts pull request comments, and `share`, which
   writes into another board.
   - `board/preference/v1` is a core type, kept in the owner's personal board.
     A rule names an operation, a path pattern, an authority (act, propose or
     ask) and how the owner is told.
   - Board settings at `/.board/settings/` are defaults. Under the `refines`
     rule a preference may narrow a board default but not widen it.
   - `board decide --op <operation> --path <path>` prints the answer and the
     rule that produced it. Board sets `BOARD_PLUGIN` in each plugin's
     environment, so it knows who asks.
   - Every `put` from a plugin runs the same check. `put --or-propose` writes
     a proposal when the answer is propose. The check binds plugins; it
     cannot stop a local process that writes files itself.
   - When the answer is ask, Board writes a pending effect at
     `/effects/pending/<id>`. Until step 4 the `put` fails with a code that
     names it. `board answer <id>` records the owner's answer as a
     `board/seen/v1` record, optionally with a rule to save, and a repeated
     `put` then proceeds. A refusal fails the `put` with
     `board::settings::refused`.
   - Left for the next slice: `required` and `overrides`, conditions beyond
     operation and path, batch `decide`, and the mandate layer.
10. **Changes to the 2026-10-05 sketch** ("Settings and the gentle slope" in
    `git show 226829b:PLAN.md`), which is otherwise background:
    - Authority gains ask, and attention is the rule's "how the owner is told".
    - A rule is a fixed operation, path pattern, authority and attention,
      not `where` and `set` over settings that manifests declare.
    - `refines` crosses layers and only narrows; the sketch let the owner
      replace a default freely.
    - `board/seen/v1` holds the answer to a pending effect, not to an offer.
11. **The gate is a trait.**

    ```rust
    pub trait Gate {
        fn ask(&self, effect: &PendingEffect) -> Result<Answer, GateError>;
    }

    pub enum Answer {
        Approve { rule: Option<Preference> },
        Propose,
        Refuse { reason: Option<String>, rule: Option<Preference> },
    }
    ```

    `rule: Some` also saves that preference. `WindowGate` comes first: it
    opens the `gate` view through `board serve` and waits. `TerminalGate`
    comes later. Because a Claude Code mod built outside viitta is a third
    implementation, the pending-effect, preference and seen records stay
    stable and documented.
12. **The gate window** shows the operation, the target, who asked and why no
    rule covers it, and a drafted rule whose parts the owner can narrow before
    saving. Its answers are approve once, approve and save the rule, and
    refuse. Saving a rule is signed through the attention plugin's Touch ID
    helper, so a process that only holds the token cannot widen the owner's
    authority. Without a signer, saving fails with a code and approving once
    still works; tests use a stub signer. A one-off approval, from the
    window, a Claude Code dialog or the phone, needs no Touch ID (decided
    2026-10-25).
