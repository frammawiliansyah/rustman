import { useEffect, useState } from "react";
import { BookOpen, ExternalLink, FileDown, FileText } from "lucide-react";
import { api } from "./api";
import { Modal, Spinner } from "./ui";
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
    <Modal title={`API Docs — ${collection.name}`} icon={BookOpen} size="docs" flush onClose={onClose} actions={<>
      <button className="btn sm" disabled={html === null} onClick={openTab}><ExternalLink size={13} />New tab</button>
      <button className="btn sm" onClick={() => exportAs("markdown")}><FileText size={13} />Markdown</button>
      <button className="btn sm" disabled={html === null} onClick={() => exportAs("html")}><FileDown size={13} />HTML</button>
    </>}>
      {err ? <div className="error-text" style={{ padding: 16 }}>{err}</div>
        : html === null ? <div className="empty-note"><Spinner size={20} /><p>Rendering…</p></div>
        // Empty sandbox: generated docs embed user-supplied text, so no scripts or same-origin access.
        : <iframe className="docs-frame" title="docs" sandbox="" srcDoc={html} />}
    </Modal>
  );
}
