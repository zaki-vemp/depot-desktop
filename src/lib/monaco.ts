/**
 * Monaco setup for the built-in code editor.
 *
 * Everything is bundled — no CDN loader — so the editor works with the app's
 * `default-src 'self'` CSP and offline. The two themes repaint Monaco's chrome
 * in the Harbor palette so a code tab does not look bolted on.
 */
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/languages/features/json/json.worker?worker";
import cssWorker from "monaco-editor/languages/features/css/css.worker?worker";
import htmlWorker from "monaco-editor/languages/features/html/html.worker?worker";
import tsWorker from "monaco-editor/languages/features/typescript/ts.worker?worker";

declare global {
  interface Window {
    MonacoEnvironment?: monaco.Environment;
  }
}

let ready = false;

/** Idempotent: worker wiring and themes are installed once per window. */
export function setupMonaco() {
  if (ready) return monaco;
  ready = true;

  window.MonacoEnvironment = {
    getWorker(_id, label) {
      if (label === "json") return new jsonWorker();
      if (label === "css" || label === "scss" || label === "less") return new cssWorker();
      if (label === "html" || label === "handlebars" || label === "razor") return new htmlWorker();
      if (label === "typescript" || label === "javascript") return new tsWorker();
      return new editorWorker();
    },
  };

  // Single files, not a project: stop the TS worker from reporting missing
  // imports and unresolved globals for a file opened in isolation.
  for (const defaults of [
    monaco.typescript.typescriptDefaults,
    monaco.typescript.javascriptDefaults,
  ]) {
    defaults.setDiagnosticsOptions({ noSemanticValidation: true, noSyntaxValidation: false });
    defaults.setCompilerOptions({
      target: monaco.typescript.ScriptTarget.ESNext,
      allowNonTsExtensions: true,
      jsx: monaco.typescript.JsxEmit.React,
    });
  }

  monaco.editor.defineTheme("depot-light", {
    base: "vs",
    inherit: true,
    rules: [],
    colors: {
      "editor.background": "#fdfdfc",
      "editor.foreground": "#1b1c1e",
      "editorGutter.background": "#fdfdfc",
      "editorLineNumber.foreground": "#bcbcb8",
      "editorLineNumber.activeForeground": "#3b74d1",
      "editor.lineHighlightBackground": "#f2f2f0",
      "editor.selectionBackground": "#dbe7fa",
      "editorIndentGuide.background1": "#eaeae7",
      "editorWidget.background": "#ffffff",
      "editorWidget.border": "#dcdcd9",
      "scrollbarSlider.background": "#dcdcd955",
    },
  });

  monaco.editor.defineTheme("depot-dark", {
    base: "vs-dark",
    inherit: true,
    rules: [],
    colors: {
      "editor.background": "#1d1e21",
      "editor.foreground": "#e9eaec",
      "editorGutter.background": "#1d1e21",
      "editorLineNumber.foreground": "#4e5258",
      "editorLineNumber.activeForeground": "#5f92e4",
      "editor.lineHighlightBackground": "#242629",
      "editor.selectionBackground": "#2c4676",
      "editorIndentGuide.background1": "#2a2c30",
      "editorWidget.background": "#242629",
      "editorWidget.border": "#34373b",
      "scrollbarSlider.background": "#34373b66",
    },
  });

  return monaco;
}

/** Extensions Monaco does not map on its own, or maps to the wrong mode. */
const EXTRA_LANGUAGES: Record<string, string> = {
  cjs: "javascript",
  mjs: "javascript",
  cts: "typescript",
  mts: "typescript",
  jsonc: "json",
  lock: "json",
  toml: "ini",
  conf: "ini",
  env: "shell",
  zsh: "shell",
  bash: "shell",
  gitignore: "plaintext",
  txt: "plaintext",
  log: "plaintext",
};

/** Filenames with no useful extension. */
const BY_NAME: Record<string, string> = {
  dockerfile: "dockerfile",
  makefile: "plaintext",
  ".gitignore": "plaintext",
  ".env": "shell",
};

export function languageForPath(path: string) {
  const name = (path.split(/[/\\]/).pop() || path).toLowerCase();
  if (BY_NAME[name]) return BY_NAME[name];

  const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : "";
  if (EXTRA_LANGUAGES[ext]) return EXTRA_LANGUAGES[ext];

  const match = monaco.languages
    .getLanguages()
    .find((lang) => lang.extensions?.some((e) => e.toLowerCase() === `.${ext}`));
  return match?.id ?? "plaintext";
}

export type { editor as MonacoEditor } from "monaco-editor";
export { monaco };
