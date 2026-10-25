import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { type Failure, fetchPage, type Page, takeCode } from "./api";
import { FailureNote, PageView } from "./Page";
import { route } from "./route";
import "./tokens.css";

takeCode();

type State = { page: Page } | { failure: Failure } | null;

function App() {
  const [path, setPath] = useState(route().path);
  const [state, setState] = useState<State>(null);
  useEffect(() => {
    const changed = () => {
      setPath(route().path);
      const anchor = route().anchor;
      if (anchor) document.getElementById(anchor)?.scrollIntoView();
    };
    addEventListener("hashchange", changed);
    return () => removeEventListener("hashchange", changed);
  }, []);
  useEffect(() => {
    const anchor = route().anchor;
    if (state && anchor) document.getElementById(anchor)?.scrollIntoView();
  });
  useEffect(() => {
    if (!path.startsWith("/bindings/")) {
      setState({
        failure: {
          code: "board::ui::not_a_binding",
          message: "The address names no binding.",
          hint: "Run `board ui open /bindings/<name>` to open a page.",
        },
      });
      return;
    }
    fetchPage(path).then(
      (page) => setState({ page }),
      (failure: Failure) => setState({ failure }),
    );
  }, [path]);
  if (state === null) return null;
  if ("page" in state) return <PageView page={state.page} />;
  return (
    <main className="mx-auto max-w-3xl px-4 py-8">
      <FailureNote failure={state.failure} />
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
