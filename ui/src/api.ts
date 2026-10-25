/** Why a slot or a page cannot be drawn, as Board reports it. */
export type Failure = { code: string; message: string; hint: string };

/** One slot of a page, as `GET /api/bindings/<name>` returns it. */
export type Drawn = {
  name: string;
  kind: string;
  ref?: string;
  data?: unknown;
  failed?: Failure;
};

export type Page = { binding: string; view: string; slots: Drawn[] };

/** The launch's token, held in memory only once the launch code is exchanged. */
let token: Promise<string> = Promise.resolve("");

/** Takes the launch code from an address of the form `#code=<hex>&/bindings/x`,
 * which `board ui open` opens, drops it from the address and history, and
 * exchanges it once for the token. A refused exchange's failure is what
 * every page request then reports. */
export function takeCode() {
  const found = /^#code=([0-9a-f]+)&(.*)$/.exec(location.hash);
  if (!found) return;
  history.replaceState(null, "", `${location.pathname}#${found[2]}`);
  token = fetch("/api/session", { method: "POST", body: found[1] }).then(async (response) => {
    if (!response.ok) throw (await response.json()) as Failure;
    return response.text();
  });
}

/** The page of the binding at `path`, such as `/bindings/review`. */
export async function fetchPage(path: string): Promise<Page> {
  const name = path.replace(/^\/bindings\//, "");
  const response = await fetch(`/api/bindings/${name}`, { headers: { Authorization: `Bearer ${await token}` } });
  const body = await response.json();
  if (!response.ok) throw body as Failure;
  return body as Page;
}
