import type { Collection, Cookie, SnippetTarget, Environment, ExecuteReply, HistoryEntry, HttpRequest } from "./types";

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, init);
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    try {
      msg = (await res.json()).error ?? msg;
    } catch { /* non-JSON error body */ }
    throw new Error(msg);
  }
  return res.status === 204 ? (undefined as T) : ((await res.json()) as T);
}

const json = (method: string, body: unknown): RequestInit => ({
  method,
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify(body),
});

export const api = {
  execute: (request: HttpRequest, environment_id: string | null) =>
    call<ExecuteReply>("/execute", json("POST", { request, environment_id })),
  collections: () => call<Collection[]>("/collections"),
  deleteCollection: (id: string) => call<void>(`/collections/${id}`, { method: "DELETE" }),
  environments: () => call<Environment[]>("/environments"),
  saveEnvironment: (e: Environment) => call<Environment>("/environments", json("POST", e)),
  deleteEnvironment: (id: string) => call<void>(`/environments/${id}`, { method: "DELETE" }),
  history: () => call<HistoryEntry[]>("/history"),
  importPostman: (raw: string) => call<Collection>("/import/postman", { method: "POST", body: raw }),
  codegen: (target: SnippetTarget, request: HttpRequest) =>
    call<{ code: string }>("/codegen", json("POST", { target, request })),
  cookies: () => call<Cookie[]>("/cookies"),
  saveCookie: (c: Cookie) => call<Cookie>("/cookies", json("POST", c)),
  deleteCookie: (domain: string, name: string) =>
    call<void>(`/cookies?domain=${encodeURIComponent(domain)}&name=${encodeURIComponent(name)}`, { method: "DELETE" }),
  importOpenapi: (raw: string) => call<Collection>("/import/openapi", { method: "POST", body: raw }),
};
