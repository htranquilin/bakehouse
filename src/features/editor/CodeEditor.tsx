import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { useEffect, useRef } from "react";
import { makeExtensions, schemaCompartment, schemaExtension, textToRun } from "./cmSetup";

interface Props {
  value: string;
  onChange: (value: string) => void;
  onRun: (sql: string) => void;
  /** Live table→columns map for autocomplete. */
  schema?: Record<string, string[]> | null;
  /** Jump target set when a messages-pane error line is clicked. */
  gotoLine?: number | null;
  onGotoHandled?: () => void;
}

export function CodeEditor({ value, onChange, onRun, schema, gotoLine, onGotoHandled }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const callbacks = useRef({ onChange, onRun });
  callbacks.current = { onChange, onRun };

  useEffect(() => {
    if (!host.current) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: value,
        extensions: [
          ...makeExtensions((v) => {
            callbacks.current.onRun(textToRun(v));
            return true;
          }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) callbacks.current.onChange(u.state.doc.toString());
          }),
        ],
      }),
      parent: host.current,
    });
    viewRef.current = view;
    return () => view.destroy();
    // The editor owns its document after mount; `value` seeds it only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !schema) return;
    view.dispatch({ effects: schemaCompartment.reconfigure(schemaExtension(schema)) });
  }, [schema]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || gotoLine == null) return;
    const line = view.state.doc.line(Math.min(gotoLine, view.state.doc.lines));
    view.dispatch({ selection: { anchor: line.from }, scrollIntoView: true });
    view.focus();
    onGotoHandled?.();
  }, [gotoLine, onGotoHandled]);

  return <div className="code-editor" ref={host} />;
}
