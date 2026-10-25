export type RecordData = Record<string, unknown>;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** A string that names a binding links to its page. */
function Scalar({ value }: { value: unknown }) {
  if (typeof value === "string") {
    if (value.startsWith("/bindings/")) {
      return (
        <a className="font-mono text-accent underline-offset-2 hover:underline" href={`#${value}`}>
          {value}
        </a>
      );
    }
    return <span className="whitespace-pre-wrap">{value}</span>;
  }
  return <span className="font-mono text-muted">{JSON.stringify(value)}</span>;
}

function Value({ value }: { value: unknown }) {
  if (Array.isArray(value) || isObject(value)) {
    const count = Array.isArray(value) ? value.length : Object.keys(value).length;
    const noun = Array.isArray(value) ? "items" : "fields";
    return (
      <details>
        <summary className="cursor-pointer text-muted select-none">
          {count} {noun}
        </summary>
        <div className="mt-1 border-l border-rule pl-3">
          <Fields value={Array.isArray(value) ? Object.fromEntries(value.entries()) : value} />
        </div>
      </details>
    );
  }
  return <Scalar value={value} />;
}

function Fields({ value }: { value: Record<string, unknown> }) {
  return (
    <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5">
      {Object.entries(value).map(([key, field]) => (
        <div key={key} className="contents">
          <dt className="font-mono text-sm text-muted">{key}</dt>
          <dd className="m-0 min-w-0">
            <Value value={field} />
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** Any record as a definition list, its `$type` as a heading and nested values folded. */
export function RecordView({ data }: { data: RecordData }) {
  const { $type, ...rest } = data;
  return (
    <div className="text-[0.9375rem]">
      {typeof $type === "string" && <p className="mb-3 font-mono text-xs text-muted">{$type}</p>}
      <Fields value={rest} />
    </div>
  );
}
