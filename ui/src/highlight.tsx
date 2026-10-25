import { toJsxRuntime } from "hast-util-to-jsx-runtime";
import type { ReactNode } from "react";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import { createHighlighterCoreSync } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import diff from "shiki/langs/diff.mjs";
import json from "shiki/langs/json.mjs";
import python from "shiki/langs/python.mjs";
import rust from "shiki/langs/rust.mjs";
import shellscript from "shiki/langs/shellscript.mjs";
import sql from "shiki/langs/sql.mjs";
import toml from "shiki/langs/toml.mjs";
import typescript from "shiki/langs/typescript.mjs";
import yaml from "shiki/langs/yaml.mjs";
import dark from "shiki/themes/vitesse-dark.mjs";
import light from "shiki/themes/vitesse-light.mjs";

// The highlighter loads a fixed set of languages synchronously, so a page
// draws in one pass and its screenshot never catches it half highlighted.
const highlighter = createHighlighterCoreSync({
  themes: [light, dark],
  langs: [diff, json, python, rust, shellscript, sql, toml, typescript, yaml],
  engine: createJavaScriptRegexEngine(),
});

/** `code` highlighted as `lang`, or as plain text when the language is not loaded. */
export function highlight(code: string, lang: string | undefined): ReactNode {
  const loaded = lang !== undefined && highlighter.getLoadedLanguages().includes(lang);
  const tree = highlighter.codeToHast(code, {
    lang: loaded ? lang : "text",
    themes: { light: "vitesse-light", dark: "vitesse-dark" },
    defaultColor: false,
  });
  return toJsxRuntime(tree, { Fragment, jsx, jsxs });
}
