import Editor, { type BeforeMount } from "@monaco-editor/react";
import { Spinner } from "./ui";

const beforeMount: BeforeMount = (monaco) => {
  monaco.editor.defineTheme("rustman-dark", {
    base: "vs-dark", inherit: true, rules: [],
    colors: {
      "editor.background": "#0f131b",
      "editor.lineHighlightBackground": "#161b2680",
      "editorLineNumber.foreground": "#4b5468",
      "editorLineNumber.activeForeground": "#a5adc0",
      "editorGutter.background": "#0f131b",
      "editor.selectionBackground": "#6366f13d",
      "editorWidget.background": "#161b26",
      "editorIndentGuide.background1": "#1e2535",
    },
  });
};

/** Monaco wrapper using the shared rustman options (vs-dark base). */
export function CodeEditor({ value, onChange, language = "json", readOnly = false, placeholder, wrap = false }: {
  value: string; onChange?: (v: string) => void; language?: string; readOnly?: boolean; placeholder?: string; wrap?: boolean;
}) {
  return (
    <div className="code-editor">
      <Editor
        height="100%" language={language} value={value} theme="rustman-dark" beforeMount={beforeMount}
        loading={<div className="editor-loading"><Spinner /> Loading editor…</div>}
        onChange={(v) => onChange?.(v ?? "")}
        options={{
          minimap: { enabled: false }, scrollBeyondLastLine: false, fontSize: 13,
          fontFamily: "JetBrains Mono, Fira Code, Menlo, monospace", lineNumbers: "on",
          automaticLayout: true, tabSize: 2, readOnly, wordWrap: wrap ? "on" : "off",
          padding: { top: 10, bottom: 10 }, renderLineHighlight: readOnly ? "none" : "line",
          smoothScrolling: true, scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10 },
          overviewRulerLanes: 0, domReadOnly: readOnly,
        }}
      />
      {placeholder && !value && <pre className="editor-placeholder">{placeholder}</pre>}
    </div>
  );
}
