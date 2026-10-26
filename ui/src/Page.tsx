import type { Drawn, Failure, Page } from "./api";
import { Diff, type DiffData } from "./kinds/Diff";
import { Markdown, type MarkdownData } from "./kinds/Markdown";
import { RecordView, type RecordData } from "./kinds/Record";

export function FailureNote({ failure }: { failure: Failure }) {
  return (
    <div role="alert" className="rounded-board border border-warn/40 bg-warn-soft px-4 py-3 text-sm text-ink">
      <p className="m-0 font-medium">{failure.message}</p>
      <p className="m-0 mt-1 text-muted">{failure.hint}</p>
      <p className="m-0 mt-2 font-mono text-xs text-warn">{failure.code}</p>
    </div>
  );
}

function Slot({ slot }: { slot: Drawn }) {
  const body = (() => {
    if (slot.failed) return <FailureNote failure={slot.failed} />;
    switch (slot.kind) {
      case "markdown":
        return <Markdown data={slot.data as MarkdownData} />;
      case "record":
        return <RecordView data={slot.data as RecordData} />;
      case "diff":
        return <Diff data={slot.data as DiffData} slot={slot.name} />;
      default:
        return <p className="text-muted">This app cannot draw the {slot.kind} kind.</p>;
    }
  })();
  return (
    <section aria-label={slot.name} className="rounded-board border border-rule bg-surface px-6 py-5">
      <header className="mb-4 flex items-baseline justify-between gap-4 text-xs text-muted">
        <span className="font-semibold tracking-wide uppercase">{slot.name}</span>
        {slot.ref && <span className="font-mono">{slot.ref}</span>}
      </header>
      {body}
    </section>
  );
}

/** A binding's page: each slot of its view, in order. */
export function PageView({ page }: { page: Page }) {
  return (
    <main className="mx-auto max-w-6xl px-4 py-8 sm:px-8">
      <header className="mb-6 flex flex-wrap items-baseline gap-x-4 gap-y-1">
        <h1 className="m-0 font-mono text-lg font-semibold">{page.binding}</h1>
        <span className="font-mono text-sm text-muted">{page.view}</span>
      </header>
      <div className="flex flex-col gap-5">
        {page.slots.map((slot) => (
          <Slot key={slot.name} slot={slot} />
        ))}
      </div>
    </main>
  );
}
