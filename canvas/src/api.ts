export type Limits = { cpu: number; ram: number; disk: number };

export type Sandbox = {
  id: string;
  name: string;
  state: "stopped" | "running";
  home: string[];
  limits: Limits;
};

export type Published = { port: number; host_port: number; url: string };

export type Template = { name: string; shipped: boolean };

export type AgentOption = {
  name: string;
  type: string;
  default: unknown;
  description: string;
};

export type AgentProgram = {
  name: string;
  description: string;
  options: AgentOption[];
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

export type Layout = {
  windows: WindowRec[];
  icon_manager: { x: number; y: number; visible: boolean };
};

async function req(path: string, init: RequestInit = {}): Promise<Response> {
  const r = await fetch(path, {
    credentials: "same-origin",
    ...init,
    headers: {
      ...(init.body ? { "content-type": "application/json" } : {}),
      ...(init.headers ?? {}),
    },
  });
  return r;
}

async function json<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await req(path, init);
  if (r.status === 204) return undefined as T;
  const body = await r.json();
  if (!r.ok) {
    throw new Error(body.detail || body.error || r.statusText);
  }
  return body as T;
}

export const api = {
  sandboxes: () => json<{ sandboxes: Sandbox[] }>("/api/v1/sandboxes"),
  create: (name?: string, template?: string) =>
    json<Sandbox>("/api/v1/sandboxes", {
      method: "POST",
      body: JSON.stringify({
        ...(name ? { name } : {}),
        ...(template ? { template } : {}),
      }),
    }),
  templates: () => json<{ templates: Template[] }>("/api/v1/templates"),
  saveTemplate: (name: string, sandbox: string) =>
    json<Template>("/api/v1/templates", {
      method: "POST",
      body: JSON.stringify({ name, sandbox }),
    }),
  published: (id: string) =>
    json<{ published: Published[] }>(`/api/v1/sandboxes/${id}/publish`),
  publish: (id: string, port: number, host_port?: number) =>
    json<Published>(`/api/v1/sandboxes/${id}/publish`, {
      method: "POST",
      body: JSON.stringify({ port, host_port: host_port ?? null }),
    }),
  unpublish: (id: string, port: number) =>
    req(`/api/v1/sandboxes/${id}/publish/${port}`, { method: "DELETE" }),
  start: (id: string) =>
    json<Sandbox>(`/api/v1/sandboxes/${id}/start`, { method: "POST" }),
  stop: (id: string) =>
    json<Sandbox>(`/api/v1/sandboxes/${id}/stop`, { method: "POST" }),
  reset: (id: string) =>
    json<Sandbox>(`/api/v1/sandboxes/${id}/reset`, { method: "POST" }),
  destroy: (id: string) =>
    req(`/api/v1/sandboxes/${id}`, { method: "DELETE" }),
  layout: () => json<Layout>("/api/v1/layout"),
  saveLayout: (layout: Layout) =>
    json<Layout>("/api/v1/layout", {
      method: "PUT",
      body: JSON.stringify(layout),
    }),
  openWindow: (sandbox: string) =>
    json<WindowRec>(`/api/v1/sandboxes/${sandbox}/windows`, { method: "POST" }),
  closeWindow: (id: string) =>
    req(`/api/v1/windows/${id}`, { method: "DELETE" }),
  patchLimits: (id: string, limits: Partial<Limits>) =>
    json<Sandbox>(`/api/v1/sandboxes/${id}`, {
      method: "PATCH",
      body: JSON.stringify({ limits }),
    }),
  agentOptions: () => json<{ programs: AgentProgram[] }>("/api/v1/agent-options"),
  environment: (id: string) => json<Record<string, unknown>>(`/api/v1/sandboxes/${id}/environment`),
  saveEnvironment: (id: string, config: unknown) =>
    json<Record<string, unknown>>(`/api/v1/sandboxes/${id}/environment`, {
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
