import { afterEach, expect, test, vi } from "vitest";
import { fetchPage, takeCode } from "./api";

afterEach(() => {
  vi.unstubAllGlobals();
});

test("takeCode drops the code from the address and history and posts it once for the token", async () => {
  const fetch = vi.fn(async (url: string) =>
    url === "/api/session" ? new Response("t0ken") : Response.json({ binding: "/bindings/x", view: "/views/v", slots: [] }),
  );
  vi.stubGlobal("fetch", fetch);
  const length = history.length;
  history.replaceState(null, "", `${location.pathname}#code=c0de&/bindings/x`);

  takeCode();

  expect(location.hash).toBe("#/bindings/x");
  expect(location.href).not.toContain("c0de");
  expect(history.length).toBe(length);
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch).toHaveBeenCalledWith("/api/session", { method: "POST", body: "c0de" });

  await fetchPage("/bindings/x");
  await fetchPage("/bindings/x");
  expect(fetch.mock.calls.filter(([url]) => url === "/api/session")).toHaveLength(1);
  expect(fetch).toHaveBeenLastCalledWith("/api/bindings/x", { headers: { Authorization: "Bearer t0ken" } });
});

test("takeCode leaves an address without a code alone and posts nothing", () => {
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  history.replaceState(null, "", `${location.pathname}#/bindings/x`);

  takeCode();

  expect(location.hash).toBe("#/bindings/x");
  expect(fetch).not.toHaveBeenCalled();
});
