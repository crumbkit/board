import type { Element, Root } from "hast";
import { isValidElement, type ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import rehypeSlug from "rehype-slug";
import remarkGfm from "remark-gfm";
import { highlight } from "../highlight";
import { anchorHref } from "../route";

export type MarkdownData = { markdown: string };

type Heading = { id: string; text: string; depth: number };

function text(node: Element | Root): string {
  return node.children
    .map((child) => (child.type === "text" ? child.value : child.type === "element" ? text(child) : ""))
    .join("");
}

/** The h2 and h3 headings of a tree, which the contents rail lists. */
function headings(into: Heading[]) {
  return () => (tree: Root) => {
    const walk = (node: Root | Element) => {
      for (const child of node.children) {
        if (child.type !== "element") continue;
        const depth = child.tagName === "h2" ? 2 : child.tagName === "h3" ? 3 : 0;
        const id = child.properties?.id;
        // remark-gfm labels the footnotes with a heading of its own.
        if (depth && typeof id === "string" && id !== "footnote-label") into.push({ id, text: text(child), depth });
        walk(child);
      }
    };
    walk(tree);
  };
}

function anchored(Tag: "h1" | "h2" | "h3" | "h4") {
  return ({ id, children }: { id?: string; children?: ReactNode }) => (
    <Tag id={id}>
      {children}
      {id && (
        <a className="anchor" href={anchorHref(id)} aria-label="Link to this heading">
          #
        </a>
      )}
    </Tag>
  );
}

/** A link within the page, such as a footnote's, keeps the binding's path
 * in the address, which the hash router reads. */
function Link({ href, children }: { href?: string; children?: ReactNode }) {
  return <a href={href?.startsWith("#") ? anchorHref(href.slice(1)) : href}>{children}</a>;
}

function Pre({ children }: { children?: ReactNode }) {
  if (!isValidElement<{ className?: string; children?: ReactNode }>(children)) return <pre>{children}</pre>;
  const lang = /language-([\w-]+)/.exec(children.props.className ?? "")?.[1];
  const code = String(children.props.children ?? "").replace(/\n$/, "");
  return <>{highlight(code, lang)}</>;
}

/** GitHub-flavoured Markdown with highlighted code, heading anchors and a contents rail. */
export function Markdown({ data }: { data: MarkdownData }) {
  const found: Heading[] = [];
  // Called as a function, react-markdown runs its plugins at once, so the
  // headings are collected before the rail below is drawn.
  const body = ReactMarkdown({
    remarkPlugins: [remarkGfm],
    rehypePlugins: [[rehypeSlug, { prefix: "heading-" }], headings(found)],
    components: { h1: anchored("h1"), h2: anchored("h2"), h3: anchored("h3"), h4: anchored("h4"), a: Link, pre: Pre },
    children: data.markdown,
  });
  return (
    <div className="flex gap-10">
      <article className="prose-board min-w-0 flex-1">{body}</article>
      {found.length > 2 && (
        <nav aria-label="Contents" className="sticky top-6 hidden w-48 shrink-0 self-start text-sm lg:block">
          <p className="mb-2 text-xs font-semibold tracking-wide text-muted uppercase">Contents</p>
          <ul className="space-y-1 border-l border-rule">
            {found.map((heading) => (
              <li key={heading.id} className={heading.depth === 3 ? "pl-6" : "pl-3"}>
                <a className="text-muted hover:text-accent" href={anchorHref(heading.id)}>
                  {heading.text}
                </a>
              </li>
            ))}
          </ul>
        </nav>
      )}
    </div>
  );
}
