export type HostRec = {
  id: string;
  url: string;
  token: string;
  label: string;
};

export type FoundHost = {
  id: string;
  addresses: string[];
  port: number;
};

export const ROSTER_KEY = "snowbox.roster";

export function normalizeUrl(raw: string): string {
  const trimmed = raw.trim().replace(/\/+$/, "");
  const withScheme = /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
  const u = new URL(withScheme);
  if (!u.port) u.port = "5418";
  u.pathname = "";
  u.search = "";
  u.hash = "";
  return u.origin;
}

export function compactRoster(hosts: HostRec[]): HostRec[] {
  let out: HostRec[] = [];
  for (const h of hosts) {
    out = upsertHost(out, h);
  }
  return out;
}

function hostUrlKey(url: string): string {
  try {
    return normalizeUrl(url);
  } catch {
    return url;
  }
}

function preferHost(a: HostRec, b: HostRec): HostRec {
  if (a.id === "origin" && b.id !== "origin") return b;
  if (b.id === "origin" && a.id !== "origin") return a;
  return b;
}

export function loadRoster(storage: Pick<Storage, "getItem"> | undefined): HostRec[] {
  if (!storage) return [];
  try {
    const raw = storage.getItem(ROSTER_KEY);
    if (!raw) return [];
    // SAFETY: saveRoster writes HostRec[]; missing fields are dropped below.
    const parsed = JSON.parse(raw) as HostRec[];
    if (!Array.isArray(parsed)) return [];
    return compactRoster(parsed.filter((h) => h?.id && h.url && h.token && h.label));
  } catch {
    return [];
  }
}

export function saveRoster(storage: Pick<Storage, "setItem">, hosts: HostRec[]): void {
  storage.setItem(ROSTER_KEY, JSON.stringify(hosts));
}

export function upsertHost(hosts: HostRec[], next: HostRec): HostRec[] {
  const row = { ...next, url: hostUrlKey(next.url) };
  const rest = hosts.filter((h) => h.id !== row.id && hostUrlKey(h.url) !== row.url);
  const prev = hosts.find((h) => h.id === row.id || hostUrlKey(h.url) === row.url);
  return [...rest, prev ? preferHost(prev, row) : row];
}

export function removeHost(hosts: HostRec[], id: string): HostRec[] {
  return hosts.filter((h) => h.id !== id);
}

export function displayTitle(host: HostRec, title: string, many: boolean): string {
  if (!many) return title;
  return `${host.label} — ${title}`;
}

export function applyDiscovery(roster: HostRec[], found: FoundHost[]): HostRec[] {
  return roster.map((h) => {
    const hit = found.find((f) => f.id === h.id);
    if (!hit) return h;
    const addr = pickAddress(hit.addresses);
    if (!addr) return h;
    const url = hostUrl(addr, hit.port);
    if (!url || url === h.url) return h;
    return { ...h, url };
  });
}

function pickAddress(addresses: string[]): string | undefined {
  const usable = addresses.filter((a) => a && a !== "127.0.0.1" && a !== "::1");
  const v4 = usable.find((a) => !a.includes(":"));
  return v4 ?? usable[0] ?? addresses.find((a) => a === "127.0.0.1") ?? addresses[0];
}

function hostUrl(addr: string, port: number): string | undefined {
  const host = addr.includes(":") ? `[${addr}]` : addr;
  try {
    return normalizeUrl(`http://${host}:${port}`);
  } catch {
    return undefined;
  }
}
