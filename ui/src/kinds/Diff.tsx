import { anchorHref } from "../route";

/** `hidden`: the text held hidden characters, which Board escaped. */
type Line = { change: "context" | "added" | "removed"; hidden?: boolean; old?: number; new?: number; text: string };
type Hunk = { header: string; lines: Line[] };
type FileDiff = {
  old?: string;
  new?: string;
  added: number;
  removed: number;
  binary: boolean;
  copied?: boolean;
  old_mode?: string;
  new_mode?: string;
  hunks: Hunk[];
};

/** A patch as Board reads it for the app: its files, hunks and numbered
 * lines, with hidden characters in paths and text already escaped. */
export type DiffData = { files: FileDiff[] };

const sign = { context: " ", added: "+", removed: "-" };
const row = {
  context: "",
  added: "bg-added-soft",
  removed: "bg-removed-soft",
};

function name(file: FileDiff): string {
  if (file.old && file.new && file.old !== file.new) return `${file.old} → ${file.new}`;
  return file.new ?? file.old ?? "";
}

function status(file: FileDiff): string | undefined {
  const mode = file.old_mode && file.new_mode ? `mode ${file.old_mode} → ${file.new_mode}` : undefined;
  if (!file.old) return "new";
  if (!file.new) return "deleted";
  if (file.binary) return "binary";
  if (file.copied) return "copied";
  if (file.old !== file.new) return mode ? `renamed, ${mode}` : "renamed";
  return mode;
}

function Counts({ file }: { file: Pick<FileDiff, "added" | "removed"> }) {
  return (
    <span className="font-mono text-xs whitespace-nowrap">
      <span className="text-added">+{file.added}</span> <span className="text-removed">−{file.removed}</span>
    </span>
  );
}

/** A line number that links to its own line. */
function LineNumber({ id, value }: { id: string; value?: number }) {
  if (value === undefined) return <td className="w-12 px-2" />;
  return (
    <td className="w-12 px-2 text-right text-muted select-none">
      <a className="hover:text-accent" href={anchorHref(id)}>
        {value}
      </a>
    </td>
  );
}

function File({ file, id }: { file: FileDiff; id: string }) {
  const label = status(file);
  return (
    <section id={id} className="overflow-hidden rounded-board border border-rule">
      <header className="flex items-baseline gap-3 border-b border-rule bg-code px-3 py-2">
        <span className="min-w-0 flex-1 truncate font-mono text-sm">{name(file)}</span>
        {label && <span className="text-xs text-muted">{label}</span>}
        <Counts file={file} />
      </header>
      {file.hunks.length > 0 && (
        <table className="w-full border-collapse font-mono text-[0.8125rem] leading-5">
          <tbody>
            {file.hunks.map((hunk, h) => [
              <tr key={`h${h}`} className="bg-accent-soft text-muted">
                <td colSpan={4} className="px-3">
                  {hunk.header}
                </td>
              </tr>,
              ...hunk.lines.map((line, l) => {
                const lineId = line.new === undefined ? `${id}-L${line.old}` : `${id}-R${line.new}`;
                return (
                  <tr key={`${h}-${l}`} id={lineId} className={`diff-line ${row[line.change]}`}>
                    <LineNumber id={lineId} value={line.old} />
                    <LineNumber id={lineId} value={line.new} />
                    <td className="w-0 pl-1 text-muted select-none">{sign[line.change]}</td>
                    <td className="pr-3 whitespace-pre-wrap break-all">
                      {line.hidden && (
                        <span
                          role="note"
                          aria-label="This line holds hidden characters, shown escaped"
                          className="mr-2 rounded-sm bg-warn-soft px-1 font-ui text-xs font-semibold text-warn"
                        >
                          hidden characters
                        </span>
                      )}
                      {line.text}
                    </td>
                  </tr>
                );
              }),
            ])}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** A unified diff: a list of its files with change counts, then each file's
 * hunks with numbered lines that link to themselves. */
export function Diff({ data, slot }: { data: DiffData; slot: string }) {
  const id = (index: number) => `diff-${slot}-${index}`;
  const total = data.files.reduce(
    (sum, file) => ({ added: sum.added + file.added, removed: sum.removed + file.removed }),
    { added: 0, removed: 0 },
  );
  return (
    <div className="flex flex-col gap-4">
      <nav aria-label="Files" className="text-sm">
        <p className="m-0 mb-2 flex gap-3 text-muted">
          <span>
            {data.files.length} {data.files.length === 1 ? "file" : "files"} changed
          </span>
          <Counts file={total} />
        </p>
        <ul className="m-0 list-none space-y-0.5 p-0">
          {data.files.map((file, index) => (
            <li key={index} className="flex items-baseline gap-3">
              <a className="min-w-0 flex-1 truncate font-mono text-accent" href={anchorHref(id(index))}>
                {name(file)}
              </a>
              <Counts file={file} />
            </li>
          ))}
        </ul>
      </nav>
      {data.files.map((file, index) => (
        <File key={index} file={file} id={id(index)} />
      ))}
    </div>
  );
}
