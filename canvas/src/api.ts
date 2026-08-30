export type Limits = { cpu: number; ram: number; disk: number };

export type Sandbox = {
  id: string;
  name: string;
  state: "stopped" | "running";
  booting?: boolean;
  home: string[];
  limits: Limits;
};

export type Published = { port: number; host_port: number; url: string };

export type Template = { name: string; shipped: boolean };

export type JsonObject = { [key: string]: Json };
export type Json = string | number | boolean | null | Json[] | JsonObject;

export type AgentOption = {
  name: string;
  type: string;
  default: Json;
  description: string;
};

export type AgentProgram = {
  name: string;
  description: string;
  options: AgentOption[];
};

export type EnvProgram = JsonObject;

export type EnvironmentDoc = {
  programs?: { [name: string]: EnvProgram };
  env?: JsonObject;
};

export type WindowRec = {
  id: string;
  sandbox: string;
  title: string;
  x: number;
  y: number;
  w: number;
  h: number;
  z: number;
  iconified: boolean;
};

export type LogRec = {
  x: number;
  y: number;
  w: number;
  h: number;
  visible: boolean;
};

export type Layout = {
  windows: WindowRec[];
  icon_manager: { x: number; y: number; w: number; h: number; visible: boolean };
  log?: LogRec;
};

declare global {
  interface Window {
    __SNOWBOX_TOKEN__?: string;
  }
}

export function sessionToken(): string | undefined {
  return globalThis.window?.__SNOWBOX_TOKEN__;
}

export function isJsonObject(value: Json | null): value is JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isNonEmptyString(value: Json): value is string {
  return typeof value === "string" && value.length > 0;
}

function errorMessage(statusText: string, body: Json | null): string {
  if (!isJsonObject(body)) return statusText;
  const detail = body.detail;
  const error = body.error;
  if (detail !== undefined && isNonEmptyString(detail)) return detail;
  if (error !== undefined && isNonEmptyString(error)) return error;
  return statusText;
}

async function req(path: string, init: RequestInit = {}): Promise<Response> {
  const token = sessionToken();
  const headers = new Headers(init.headers);
  if (token) headers.set("authorization", `Bearer ${token}`);
  if (init.body && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  return fetch(path, {
    credentials: "same-origin",
    ...init,
    headers,
  });
}

async function failIfNotOk(r: Response): Promise<void> {
  if (r.ok) return;
  let body: Json | null = null;
  try {
    // SAFETY: error bodies are JSON objects with optional detail/error strings.
    body = (await r.json()) as Json;
  } catch {
    /* 4xx/5xx may have an empty body */
  }
  throw new Error(errorMessage(r.statusText, body));
}

/** JSON body. 204 is an error here — use `empty` for no-content routes. */
export async function json<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await req(path, init);
  await failIfNotOk(r);
  if (r.status === 204) {
    throw new Error("unexpected empty body");
  }
  const text = await r.text();
  try {
    // SAFETY: JSON.parse is untyped; T is the Daemon API contract for this path.
    return JSON.parse(text) as T;
  } catch {
    throw new Error("expected JSON");
  }
}

/** 204 / no JSON body. */
export async function empty(path: string, init?: RequestInit): Promise<void> {
  const r = await req(path, init);
  await failIfNotOk(r);
}

function createSandboxBody(name?: string, template?: string, environment?: EnvironmentDoc) {
  if (name && template && environment) return { name, template, environment };
  if (name && template) return { name, template };
  if (name && environment) return { name, environment };
  if (template && environment) return { template, environment };
  if (name) return { name };
  if (template) return { template };
  if (environment) return { environment };
  return {};
}

export const api = {
  sandboxes: () => json<{ sandboxes: Sandbox[] }>("/api/v1/sandboxes"),
  create: (name?: string, template?: string, environment?: EnvironmentDoc) => {
    const body = createSandboxBody(name, template, environment);
    return json<Sandbox>("/api/v1/sandboxes", {
      method: "POST",
      body: JSON.stringify(body),
    });
  },
  templates: () => json<{ templates: Template[] }>("/api/v1/templates"),
  template: (name: string) => json<EnvironmentDoc>(`/api/v1/templates/${name}`),
  saveTemplateConfig: (name: string, config: EnvironmentDoc) =>
    json<EnvironmentDoc>(`/api/v1/templates/${name}`, {
      method: "PUT",
      body: JSON.stringify(config),
    }),
  saveTemplate: (name: string, sandbox: string) =>
    json<Template>("/api/v1/templates", {
      method: "POST",
      body: JSON.stringify({ name, sandbox }),
    }),
  published: (id: string) => json<{ published: Published[] }>(`/api/v1/sandboxes/${id}/publish`),
  publish: (id: string, port: number, host_port?: number) =>
    json<Published>(`/api/v1/sandboxes/${id}/publish`, {
      method: "POST",
      body: JSON.stringify({ port, host_port: host_port ?? null }),
    }),
  unpublish: (id: string, port: number) =>
    empty(`/api/v1/sandboxes/${id}/publish/${port}`, { method: "DELETE" }),
  start: (id: string) => json<Sandbox>(`/api/v1/sandboxes/${id}/start`, { method: "POST" }),
  stop: (id: string) => json<Sandbox>(`/api/v1/sandboxes/${id}/stop`, { method: "POST" }),
  reset: (id: string) => json<Sandbox>(`/api/v1/sandboxes/${id}/reset`, { method: "POST" }),
  destroy: (id: string) => empty(`/api/v1/sandboxes/${id}`, { method: "DELETE" }),
  layout: () => json<Layout>("/api/v1/layout"),
  saveLayout: (layout: Layout) =>
    json<Layout>("/api/v1/layout", {
      method: "PUT",
      body: JSON.stringify(layout),
    }),
  openWindow: (sandbox: string) =>
    json<WindowRec>(`/api/v1/sandboxes/${sandbox}/windows`, { method: "POST" }),
  closeWindow: (id: string) => empty(`/api/v1/windows/${id}`, { method: "DELETE" }),
  patchLimits: (id: string, limits: Partial<Limits>) =>
    json<Sandbox>(`/api/v1/sandboxes/${id}`, {
      method: "PATCH",
      body: JSON.stringify({ limits }),
    }),
  agentOptions: () => json<{ programs: AgentProgram[] }>("/api/v1/agent-options"),
  progress: () => json<{ lines: string[] }>("/api/v1/progress"),
  environment: (id: string) => json<EnvironmentDoc>(`/api/v1/sandboxes/${id}/environment`),
  saveEnvironment: (id: string, config: EnvironmentDoc) =>
    json<EnvironmentDoc>(`/api/v1/sandboxes/${id}/environment`, {
      method: "PUT",
      body: JSON.stringify(config),
    }),
  copyIn: (id: string, from: string, replace: boolean) =>
    json<Sandbox>(`/api/v1/sandboxes/${id}/copy-in`, {
      method: "POST",
      body: JSON.stringify({ from, replace }),
    }),
  copyOut: (id: string, to: string, replace: boolean) =>
    json<Sandbox>(`/api/v1/sandboxes/${id}/copy-out`, {
      method: "POST",
      body: JSON.stringify({ to, replace }),
    }),
};
