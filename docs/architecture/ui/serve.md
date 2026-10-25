# board serve

This file holds detail for docs/architecture.md. It says how `board serve`
serves pages and how the app is built.

## Server

- `board-serve` uses `tiny_http` on one thread; there is no async runtime.
- It binds `127.0.0.1` on a port the system picks.
- It writes `server.json` (`port`, `token`) in the board's cache directory,
  which it makes mode 0700, with the file mode 0600. It removes the file on
  exit, including on SIGINT, SIGTERM or SIGHUP, if it still holds its own
  record. A second signal exits at once with status 1 and leaves the file.
- It exits after `--idle` without an API request that carries the token, 30
  minutes by default, then prints the port and the number of requests.
- No answer depends on it. Every request reads the working tree afresh.
- `tiny_http` limits neither header length nor how long it waits for a
  request, so a local process can hold the one thread or send a huge header.

## Requests

1. The `Host` header must be `127.0.0.1:<port>`, else 403
   `board::serve::host`. This stops a page reaching the server through a DNS
   name that resolves to localhost.
2. Only the routes below are answered; any other is 404
   `board::serve::unknown_route`.

Routes:

- `GET /`, `/assets/app.js` and `/assets/app.css`: the app, embedded from
  `ui/dist` with `include_bytes!`. They need no token.
- `GET /api/probe?nonce=<hex>`: SHA-256 of the token followed by the nonce,
  so `board ui open` can check a server holds the recorded token without
  sending it.
- `POST /api/codes`, with a binding's path as the body: a launch code
  (STANDARDS.md 28) for that page. The server writes an opener page holding
  it and replies with the opener's path. It needs the bearer token.
- `POST /api/session`, with a launch code as the body: the token. Each code
  is 32 random bytes and works once, within 60 s. The server keeps only its
  SHA-256, and removes its opener when it is used or expires. A refused code
  is 401 `board::serve::code_unknown`, `code_used` or `code_expired`.
- Both `POST` routes refuse a request whose `Sec-Fetch-Site` is
  `cross-site`, or whose `Origin` is not `http://127.0.0.1:<port>`, with
  403 `board::serve::origin`.
- `GET /api/bindings/<name>`: the page of `/bindings/<name>`. It needs
  `Authorization: Bearer <token>`, else 401 `board::serve::token`. Tokens
  are 32 random bytes, compared in constant time and never printed. It reads
  the binding and its view in one snapshot and gives each slot `name`,
  `kind`, `ref`, and its `data` or the record it references. A slot that
  cannot be drawn carries `failed` (`code`, `message`, `hint`) and the rest
  still draw. A path that is missing is 404; one that holds no binding is
  422.

Every reply has `Cache-Control: no-store`, `nosniff`, `no-referrer`,
same-origin `Cross-Origin-Resource-Policy` and `Cross-Origin-Opener-Policy`,
and a CSP that allows only the server's own scripts and styles, with no base,
form or object targets.

## Opening a page

`board ui open` reuses the server when it answers a probe with the right
proof. It refuses `server.json` with `board::ui::open_failed` unless it
is a regular file of this user with mode 0600 and one link, in a directory
of this user with mode 0700. It checks before it opens the file, so it
never opens a link or a FIFO, and refuses an open file that is not the one
it checked.
With no live server it starts `board serve` detached and waits up to 5 s.
It then asks for a launch code with the token, which also restarts the
server's idle timer before the browser arrives. The server writes a new
`open-<random>.html`, mode 0600, beside `server.json`, and removes openers
older than a minute. The opener sends the browser to
`http://127.0.0.1:<port>/#code=…&/bindings/<name>`. `board ui open` opens
the opener's percent-encoded `file://` URL with `$BROWSER`, or `open` on
macOS and `xdg-open` elsewhere. `$BROWSER` names one program, which gets
the URL as its only argument. A snap or Flatpak browser cannot read hidden
directories in the home directory, such as `~/.cache`, so `ui open` may fail
with one. The server removes every opener it still holds when it exits. The
app drops the code from the address,
exchanges it for the token, keeps the token in memory and sends it as a
bearer token. A reload loses it; `board ui open` again. Failing to start either program is `board::ui::open_failed`.

## The app

- `ui/` holds board-ui: React, Vite, Tailwind, Shiki and Storybook, managed
  with pnpm. Its tokens are in `ui/src/tokens.css`, light and dark.
- The built bundle, `ui/dist`, is committed, so the board binary builds with
  only Rust. `bin/lint ui` rebuilds it and fails if it changed.
- Each kind has a component, stories with fixtures, and a golden screenshot
  per story in each theme under `ui/goldens/`. `bin/ui-goldens` compares
  them, and `bin/ui-goldens --update` rewrites them.
- Builds and screenshots run only in the pinned arm64 Playwright image
  (`bin/ui-container`), so every machine and CI give the same pixels.
- Code is highlighted for a fixed set of languages, chosen to keep the
  bundle under 1 MB. Mermaid diagrams are drawn as code.
