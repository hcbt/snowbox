import { normalizeUrl, type HostRec } from "./hosts";

export type Limits = { cpu: number; ram: number; disk: number };

export type Sandbox = {
  id: string;
  name: string;
  state: "stopped" | "running";
  booting?: boolean;
  home: string[];
  limits: Limits;
  host?: string;
};

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
  host?: string;
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

export type Scope = { base: string; token: string };

export function originScope(): Scope {
  return { base: "", token: sessionToken() ?? "" };
}

export function hostScope(url: string, token: string): Scope {
  return { base: url.replace(/\/+$/, ""), token };
}

export async function attachHost(url: string, token: string): Promise<HostRec> {
  const base = normalizeUrl(url);
  const rec = await json<{ id: string }>("/api/v1/host", undefined, hostScope(base, token));
  let label = base;
  try {
    label = new URL(base).hostname;
  } catch {
    /* keep origin */
  }
  return { id: rec.id, url: base, token, label };
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

async function req(
  path: string,
  init: RequestInit = {},
  scope: Scope = originScope(),
): Promise<Response> {
  const headers = new Headers(init.headers);
  if (scope.token) headers.set("authorization", `Bearer ${scope.token}`);
  if (init.body && !headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  const url = path.startsWith("http") ? path : `${scope.base}${path}`;
  return fetch(url, {
    credentials: scope.base ? "omit" : "same-origin",
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
export async function json<T>(path: string, init?: RequestInit, scope?: Scope): Promise<T> {
  const r = await req(path, init, scope);
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
export async function empty(path: string, init?: RequestInit, scope?: Scope): Promise<void> {
  const r = await req(path, init, scope);
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

export function apiOn(scope: Scope = originScope()) {
  return {
    host: () => json<{ id: string }>("/api/v1/host", undefined, scope),
    discovery: () =>
      json<{ hosts: { id: string; addresses: string[]; port: number }[] }>(
        "/api/v1/discovery",
        undefined,
        scope,
      ),
    sandboxes: () => json<{ sandboxes: Sandbox[] }>("/api/v1/sandboxes", undefined, scope),
    create: (name?: string, template?: string, environment?: EnvironmentDoc) => {
      const body = createSandboxBody(name, template, environment);
      return json<Sandbox>(
        "/api/v1/sandboxes",
        {
          method: "POST",
          body: JSON.stringify(body),
        },
        scope,
      );
    },
    templates: () => json<{ templates: Template[] }>("/api/v1/templates", undefined, scope),
    template: (name: string) => json<EnvironmentDoc>(`/api/v1/templates/${name}`, undefined, scope),
    saveTemplateConfig: (name: string, config: EnvironmentDoc) =>
      json<EnvironmentDoc>(
        `/api/v1/templates/${name}`,
        {
          method: "PUT",
          body: JSON.stringify(config),
        },
        scope,
      ),
    saveTemplate: (name: string, sandbox: string) =>
      json<Template>(
        "/api/v1/templates",
        {
          method: "POST",
          body: JSON.stringify({ name, sandbox }),
        },
        scope,
      ),
    start: (id: string) =>
      json<Sandbox>(`/api/v1/sandboxes/${id}/start`, { method: "POST" }, scope),
    stop: (id: string) => json<Sandbox>(`/api/v1/sandboxes/${id}/stop`, { method: "POST" }, scope),
    reset: (id: string) =>
      json<Sandbox>(`/api/v1/sandboxes/${id}/reset`, { method: "POST" }, scope),
    destroy: (id: string) => empty(`/api/v1/sandboxes/${id}`, { method: "DELETE" }, scope),
    layout: () => json<Layout>("/api/v1/layout", undefined, scope),
    saveLayout: (layout: Layout) =>
      json<Layout>(
        "/api/v1/layout",
        {
          method: "PUT",
          body: JSON.stringify(layout),
        },
        scope,
      ),
    openWindow: (sandbox: string) =>
      json<WindowRec>(`/api/v1/sandboxes/${sandbox}/windows`, { method: "POST" }, scope),
    closeWindow: (id: string) => empty(`/api/v1/windows/${id}`, { method: "DELETE" }, scope),
    patchLimits: (id: string, limits: Partial<Limits>) =>
      json<Sandbox>(
        `/api/v1/sandboxes/${id}`,
        {
          method: "PATCH",
          body: JSON.stringify({ limits }),
        },
        scope,
      ),
    agentOptions: () =>
      json<{ programs: AgentProgram[] }>("/api/v1/agent-options", undefined, scope),
    progress: () => json<{ lines: string[] }>("/api/v1/progress", undefined, scope),
    environment: (id: string) =>
      json<EnvironmentDoc>(`/api/v1/sandboxes/${id}/environment`, undefined, scope),
    saveEnvironment: (id: string, config: EnvironmentDoc) =>
      json<EnvironmentDoc>(
        `/api/v1/sandboxes/${id}/environment`,
        {
          method: "PUT",
          body: JSON.stringify(config),
        },
        scope,
      ),
  };
}

export const api = apiOn();
