import { useEffect, useState } from "react";
import { api } from "./api";
import { download } from "./download";
import type { Collection } from "./types";

const slug = (s: string) => s.replace(/[^\w.-]+/g, "_") || "collection";

export function DocsModal({ collection, onClose }: { collection: Collection; onClose: () => void }) {
  const [html, setHtml] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api.renderDocs(collection, "html").then((r) => live && setHtml(r.content)).catch((e) => live && setErr((e as Error).message));
    return () => { live = false; };
  }, [collection]);

  const exportAs = async (fmt: "html" | "markdown") => {
    try {
      const content = fmt === "html" && html !== null ? html : (await api.renderDocs(collection, fmt)).content;
      download(`${slug(collection.name)}.${fmt === "html" ? "html" : "md"}`, content, fmt === "html" ? "text/html" : "text/markdown");
    } catch (e) { setErr((e as Error).message); }
  };
  const openTab = () => {
    if (html === null) return;
    const url = URL.createObjectURL(new Blob([html], { type: "text/html" }));
    window.open(url, "_blank", "noopener");
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  };

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal docs" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <b>API Docs — {collection.name}</b>
          <span className="actions" style={{ margin: 0 }}>
            <button disabled={html === null} onClick={openTab}>Open in new tab</button>
            <button onClick={() => exportAs("markdown")}>Export Markdown</button>
            <button disabled={html === null} onClick={() => exportAs("html")}>Export HTML</button>
            <button className="icon" onClick={onClose}>×</button>
          </span>
        </div>
        {err ? <div className="error-text" style={{ padding: 12 }}>{err}</div>
          : html === null ? <div className="muted">Rendering…</div>
          // Empty sandbox: generated docs embed user-supplied text, so no scripts or same-origin access.
          : <iframe className="docs-frame" title="docs" sandbox="" srcDoc={html} />}
      </div>
    </div>
  );
}
