/** The address's hash holds the binding's path and, after a second `#`, a
 * heading on its page: `#/bindings/review#summary`. */
export function route(): { path: string; anchor?: string } {
  const [path, anchor] = location.hash.slice(1).split("#");
  return { path: path ?? "", anchor };
}

/** A link to the heading `id` on the page being shown. */
export function anchorHref(id: string): string {
  return `#${route().path}#${id}`;
}
