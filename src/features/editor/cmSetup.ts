// CodeMirror 6 configuration: T-SQL dialect, brand-token theme, run keymap.

import { autocompletion, completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { MSSQL, sql, type SQLNamespace } from "@codemirror/lang-sql";
import { bracketMatching, syntaxHighlighting, HighlightStyle } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";

const v = (name: string) => `var(--bh-${name})`;

const theme = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "var(--bh-text-code)",
    backgroundColor: v("bg"),
    color: v("text"),
  },
  ".cm-content": {
    fontFamily: "var(--bh-font-mono)",
    fontVariantLigatures: "none",
    caretColor: v("syn-cursor"),
  },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: v("syn-cursor") },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground":
    { backgroundColor: v("syn-selection") },
  ".cm-activeLine": { backgroundColor: v("syn-line") },
  ".cm-gutters": {
    backgroundColor: v("bg"),
    color: v("text-faint"),
    border: "none",
    fontFamily: "var(--bh-font-mono)",
  },
  ".cm-activeLineGutter": { backgroundColor: v("syn-line"), color: v("text-muted") },
  ".cm-tooltip": {
    backgroundColor: v("surface-raised"),
    color: v("text"),
    border: `1px solid ${v("border")}`,
    borderRadius: "var(--bh-radius-field)",
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: v("syn-selection"),
    color: v("text"),
  },
});

const highlight = HighlightStyle.define([
  { tag: tags.keyword, color: v("syn-keyword") },
  { tag: tags.string, color: v("syn-string") },
  { tag: tags.number, color: v("syn-number") },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: v("syn-function") },
  { tag: tags.comment, color: v("syn-comment"), fontStyle: "italic" },
  { tag: tags.operator, color: v("syn-operator") },
  { tag: [tags.typeName, tags.className], color: v("syn-function") },
]);

export const schemaCompartment = new Compartment();

export function makeExtensions(onRun: (view: EditorView) => boolean): Extension[] {
  return [
    lineNumbers(),
    highlightActiveLineGutter(),
    history(),
    drawSelection(),
    EditorState.allowMultipleSelections.of(true),
    bracketMatching(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    autocompletion(),
    keymap.of([
      { key: "Mod-Enter", run: onRun },
      ...defaultKeymap,
      ...historyKeymap,
      ...searchKeymap,
      ...completionKeymap,
      indentWithTab,
    ]),
    sql({ dialect: MSSQL, upperCaseKeywords: true }),
    schemaCompartment.of([]),
    theme,
    syntaxHighlighting(highlight),
  ];
}

/** Live schema completions (fed from the object tree in M5). */
export function schemaExtension(schema: SQLNamespace): Extension {
  return sql({ dialect: MSSQL, upperCaseKeywords: true, schema }).support;
}

/** ⌘↵ semantics: selection if any, else the whole buffer. */
export function textToRun(view: EditorView): string {
  const { state } = view;
  const sel = state.selection.main;
  return sel.empty ? state.doc.toString() : state.sliceDoc(sel.from, sel.to);
}
