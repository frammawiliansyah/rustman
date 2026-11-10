// Mirrors the serde JSON representation of rustman-core models.
export type Method = "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS";

export interface KV { key: string; value: string; enabled: boolean }

export type RequestBody =
  | "None"
  | { Raw: { content_type: string; body: string } }
  | { FormUrlEncoded: KV[] }
  | { FormData: KV[] };

export type AuthConfig =
  | "None"
  | { Bearer: { token: string } }
  | { Basic: { username: string; password: string } }
  | { ApiKey: { key: string; value: string; in_header: boolean } };

export interface HttpRequest {
  id: string;
  name: string;
  method: Method;
  url: string;
  headers: KV[];
  query_params: KV[];
  body: RequestBody;
  auth: AuthConfig;
  pre_request_script?: string | null;
  test_script?: string | null;
}

export interface HttpResponse {
  status: number;
  headers: KV[];
  body: string;
  elapsed_ms: number;
  timestamp: string;
}

export interface Timing {
  dns_ms: number | null;
  tcp_ms: number | null;
  tls_ms: number | null;
  ttfb_ms: number | null;
  download_ms: number | null;
  total_ms: number;
}

export interface ExecuteReply { response: HttpResponse; timing: Timing; history_id: string | null }

export interface Folder { id: string; name: string; description?: string | null; items: Item[] }
export type Item = { Request: HttpRequest } | { Folder: Folder };
export interface Collection { id: string; name: string; description?: string | null; items: Item[] }

export interface Variable { key: string; value: string; enabled: boolean; secret: boolean }
export interface Environment { id: string; name: string; variables: Variable[] }

export interface HistoryEntry {
  id: string;
  method: string;
  url: string;
  status: number;
  elapsed_ms: number;
  request: HttpRequest;
  response: HttpResponse;
  created_at: string;
}

export type SnippetTarget = "curl" | "javascript" | "python" | "go" | "rust";

export interface Cookie {
  name: string; value: string; domain: string; path: string;
  expires?: string | null; http_only: boolean; secure: boolean;
}

export interface WsMessage {
  direction: "In" | "Out"; payload_type: "Text" | "Binary"; content: string; timestamp: string;
}
