import { useEffect, useState } from "react";
import { Code2 } from "lucide-react";
import { api } from "./api";
import { CodeEditor } from "./CodeEditor";
import { CopyButton, Modal, Tabs } from "./ui";
import type { HttpRequest, SnippetTarget } from "./types";

const TARGETS: [SnippetTarget, string, string][] = [
  ["curl", "cURL", "shell"], ["javascript", "JavaScript Fetch", "javascript"], ["python", "Python Requests", "python"],
  ["go", "Go net/http", "go"], ["rust", "Rust reqwest", "rust"],
];

export function CodeModal({ req, onClose }: { req: HttpRequest; onClose: () => void }) {
  const [target, setTarget] = useState<SnippetTarget>("curl");
  const [code, setCode] = useState("");
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setErr(null);
    api.codegen(target, req)
      .then((r) => live && setCode(r.code))
      .catch((e) => live && (setCode(""), setErr((e as Error).message)));
    return () => { live = false; };
  }, [target, req]);

  return (
    <Modal title="Code snippet" icon={Code2} size="wide" onClose={onClose} actions={<CopyButton text={code} label="Copy" />}>
      <Tabs value={target} onChange={setTarget} items={TARGETS.map(([key, label]) => ({ key, label }))} />
      {err ? <div className="error-text">{err}</div> : (
        <div className="snippet"><CodeEditor language={TARGETS.find((t) => t[0] === target)![2]} value={code} readOnly wrap /></div>
      )}
    </Modal>
  );
}
