import type { ReactNode } from "react";

const TOKEN = /("(?:\\.|[^"\\])*")(\s*:)?|\b(true|false)\b|\bnull\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g;

/** Pretty-prints JSON (when valid) and highlights tokens without using innerHTML. */
export function JsonView({ text }: { text: string }) {
  let pretty = text;
  let isJson = false;
  try {
    pretty = JSON.stringify(JSON.parse(text), null, 2);
    isJson = true;
  } catch { /* show raw text */ }
  if (!isJson) return <pre className="code">{text}</pre>;

  const out: ReactNode[] = [];
  let last = 0;
  for (const m of pretty.matchAll(TOKEN)) {
    const i = m.index!;
    if (i > last) out.push(pretty.slice(last, i));
    const cls = m[1] ? (m[2] ? "j-key" : "j-str") : m[3] ? "j-bool" : m[0] === "null" ? "j-null" : "j-num";
    out.push(<span key={i} className={cls}>{m[1] && m[2] ? m[1] : m[0]}</span>);
    if (m[1] && m[2]) out.push(m[2]);
    last = i + m[0].length;
  }
  out.push(pretty.slice(last));
  return <pre className="code">{out}</pre>;
}
