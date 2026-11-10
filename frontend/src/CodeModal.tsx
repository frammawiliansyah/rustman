import { useEffect, useState } from "react";
import { api } from "./api";
import type { HttpRequest, SnippetTarget } from "./types";

const TARGETS: [SnippetTarget, string][] = [
  ["curl", "cURL"], ["javascript", "JavaScript Fetch"], ["python", "Python Requests"],
  ["go", "Go net/http"], ["rust", "Rust reqwest"],
];

export function CodeModal({ req, onClose }: { req: HttpRequest; onClose: () => void }) {
  const [target, setTarget] = useState<SnippetTarget>("curl");
  const [code, setCode] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let live = true;
    setErr(null); setCopied(false);
    api.codegen(target, req)
      .then((r) => live && setCode(r.code))
      .catch((e) => live && (setCode(""), setErr((e as Error).message)));
    return () => { live = false; };
  }, [target, req]);

  const copy = async () => {
    try { await navigator.clipboard.writeText(code); setCopied(true); setTimeout(() => setCopied(false), 1500); }
    catch { setErr("Clipboard unavailable"); }
  };

  return (
    <div className="modal-bg" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head"><b>Code snippet</b><button className="icon" onClick={onClose}>×</button></div>
        <div className="modal-body block">
          <div className="tabs">
            {TARGETS.map(([k, l]) => (
              <button key={k} className={target === k ? "active" : ""} onClick={() => setTarget(k)}>{l}</button>
            ))}
          </div>
          {err ? <div className="error-text">{err}</div> : <pre className="code snippet">{code}</pre>}
          <div className="actions"><button onClick={copy} disabled={!code}>{copied ? "Copied!" : "Copy to Clipboard"}</button></div>
        </div>
      </div>
    </div>
  );
}
