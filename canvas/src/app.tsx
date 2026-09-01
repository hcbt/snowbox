import { For, Show, createEffect, createSignal, flush, onSettled } from "solid-js";
import { Term } from "./term";
import { Frame } from "./frame";
import { RootMenu } from "./menu";
import { OverlayDialog } from "./dialogs";
import { overlayZ, placeOverlay, type Overlay } from "./overlay";
import {
  api,
  apiOn,
  attachHost,
  hostScope,
  sessionToken,
  type Layout,
  type Sandbox,
  type WindowRec,
} from "./api";
import {
  applyDiscovery,
  displayTitle,
  loadRoster,
  removeHost,
  saveRoster,
  upsertHost,
  type HostRec,
} from "./hosts";
import { sandboxesWithoutWindows } from "./sandbox-icons";
import { menuHit, type MenuHit } from "./menu-target";
import {
  applyFetchedLayout,
  defaultLayout,
  defaultLog,
  readCachedSnap,
  sameLayout,
  sameLines,
  sameSandboxes,
  sandboxLive,
  writeCachedLayout,
  writeCachedLogLines,
  writeCachedSandboxes,
} from "./layout-sync";

const hostLayouts = new Map<string, Layout>();

function persistRoster(hosts: HostRec[]): void {
  const storage = globalThis.localStorage;
  if (!storage) return;
  saveRoster(storage, hosts);
}

function clientFor(hosts: HostRec[], hostId?: string) {
  const h = hosts.find((x) => x.id === hostId) ?? hosts[0];
  if (!h) return api;
  return apiOn(hostScope(h.url, h.token));
}

function overlayId(ov: Overlay): string | undefined {
  if ("id" in ov) return ov.id;
  return undefined;
}

function hostById(hosts: HostRec[], id?: string): HostRec | undefined {
  return hosts.find((h) => h.id === id) ?? hosts[0];
}

function focusTerm(id: string): void {
  const el = document.querySelector(`[data-win="${id}"] textarea.xterm-helper-textarea`);
  if (el instanceof HTMLTextAreaElement) {
    el.focus({ preventScroll: true });
  }
}

export function App() {
  const cached = readCachedSnap();
  const [sandboxes, setSandboxes] = createSignal<Sandbox[]>(cached?.sandboxes ?? []);
  const [layout, setLayout] = createSignal<Layout>(cached?.layout ?? defaultLayout());
  const [hosts, setHosts] = createSignal<HostRec[]>(loadRoster(globalThis.localStorage));
  const [focus, setFocus] = createSignal<string | null>(null);
  const [menu, setMenu] = createSignal<MenuHit | null>(null);
  const [overlay, setOverlay] = createSignal<Overlay | null>(null);
  const [status, setStatus] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [ready, setReady] = createSignal(cached !== null);
  const [interacting, setInteracting] = createSignal(false);
  const [logLines, setLogLines] = createSignal<string[]>(cached?.logLines ?? []);

  const commit = (next: Layout) => {
    setLayout(next);
    writeCachedLayout(next);
  };

  const logRec = () => layout().log ?? defaultLog();

  const setRoster = (next: HostRec[]) => {
    setHosts(next);
    persistRoster(next);
  };

  const loadOrigin = async () => {
    const [s, l] = await Promise.all([api.sandboxes(), api.layout()]);
    if (!sameSandboxes(sandboxes(), s.sandboxes)) {
      setSandboxes(s.sandboxes);
      writeCachedSandboxes(s.sandboxes);
    }
    const ids = new Set(s.sandboxes.map((b) => b.id));
    const next = applyFetchedLayout(layout(), l, ids, interacting());
    if (!sameLayout(layout(), next)) commit(next);
    setReady(true);
  };

  const refresh = async () => {
    let roster = hosts();
    const tok = sessionToken();
    if (tok) {
      try {
        roster = upsertHost(roster, await attachHost(globalThis.location.origin, tok));
        setRoster(roster);
      } catch {
        /* loopback token is enough to call this origin without a roster row */
      }
    }
    if (roster.length === 0) {
      if (!tok) {
        setReady(true);
        setOverlay((cur) => cur ?? { kind: "attach", ...placeOverlay() });
        return;
      }
      await loadOrigin();
      return;
    }
    try {
      const found = await clientFor(roster).discovery();
      roster = applyDiscovery(roster, found.hosts);
      setRoster(roster);
    } catch {
      /* Discovery is optional */
    }
    const many = roster.length > 1;
    const parts = await Promise.all(
      roster.map(async (h) => {
        const c = apiOn(hostScope(h.url, h.token));
        try {
          const [s, l] = await Promise.all([c.sandboxes(), c.layout()]);
          return { h, sandboxes: s.sandboxes, layout: l, ok: true as const };
        } catch (e) {
          const none: Sandbox[] = [];
          return { h, sandboxes: none, layout: defaultLayout(), ok: false as const, err: e };
        }
      }),
    );
    const failed = parts.filter((p) => !p.ok);
    if (failed.length > 0) {
      setStatus(failed.map((p) => `${p.h.label}: unreachable`).join("; "));
    }
    const allSb: Sandbox[] = [];
    const allWin: WindowRec[] = [];
    const prevSb = sandboxes();
    const prevWin = layout().windows;
    for (const p of parts) {
      if (!p.ok) {
        for (const sb of prevSb) {
          if (sb.host === p.h.id) allSb.push(sb);
        }
        for (const w of prevWin) {
          if (w.host === p.h.id) allWin.push(w);
        }
        continue;
      }
      hostLayouts.set(p.h.id, p.layout);
      for (const sb of p.sandboxes) allSb.push({ ...sb, host: p.h.id });
      for (const w of p.layout.windows) {
        allWin.push({
          ...w,
          host: p.h.id,
          title: displayTitle(p.h, w.title, many),
        });
      }
    }
    if (!sameSandboxes(sandboxes(), allSb)) {
      setSandboxes(allSb);
      writeCachedSandboxes(allSb);
    }
    const ids = new Set(allSb.map((b) => b.id));
    const chrome = parts.find((p) => p.ok)?.layout ?? defaultLayout();
    const fetched: Layout = {
      windows: allWin,
      icon_manager: chrome.icon_manager,
      log: chrome.log,
    };
    const next = applyFetchedLayout(layout(), fetched, ids, interacting());
    if (!sameLayout(layout(), next)) commit(next);
    setReady(true);
  };

  const live = (sandboxId: string) => sandboxLive(sandboxes(), sandboxId);

  onSettled(() => {
    refresh().catch((e) => setStatus(String(e)));
    const t = setInterval(() => {
      refresh().catch(() => {});
    }, 2000);
    return () => clearInterval(t);
  });

  const save = async (next: Layout) => {
    if (!ready()) return;
    commit(next);
    const roster = hosts();
    const chromeId = roster[0]?.id;
    if (roster.length === 0) {
      try {
        await api.saveLayout(next);
      } catch (e) {
        setStatus(String(e));
      }
      return;
    }
    try {
      await Promise.all(
        roster.map((h) => {
          const prev = hostLayouts.get(h.id) ?? defaultLayout();
          const windows = next.windows
            .filter((w) => w.host === h.id || (!w.host && h.id === chromeId))
            .map((w) => {
              const { host: _host, ...rest } = w;
              const raw = prev.windows.find((p) => p.id === rest.id);
              return { ...rest, title: raw?.title ?? rest.title };
            });
          const layoutForHost: Layout = {
            windows,
            icon_manager: h.id === chromeId ? next.icon_manager : prev.icon_manager,
            log: h.id === chromeId ? next.log : prev.log,
          };
          return apiOn(hostScope(h.url, h.token)).saveLayout(layoutForHost);
        }),
      );
    } catch (e) {
      setStatus(String(e));
    }
  };

  const patchWin = (id: string, patch: Partial<WindowRec>, persist = false) => {
    const next: Layout = {
      ...layout(),
      windows: layout().windows.map((w) => (w.id === id ? { ...w, ...patch } : w)),
      log: logRec(),
    };
    commit(next);
    if (persist) void save(next);
  };

  const beginGeom = () => setInteracting(true);
  const endGeom = () => {
    setInteracting(false);
    flush();
    void save(layout());
  };

  const raise = (id: string) => {
    setFocus(id);
    focusTerm(id);
    const z = Math.max(0, ...layout().windows.map((w) => w.z));
    const cur = layout().windows.find((w) => w.id === id);
    if (!cur || cur.z === z) return;
    patchWin(id, { z: z + 1 }, false);
    queueMicrotask(() => focusTerm(id));
  };

  const run = async (fn: () => Promise<void>, done = "", log = false) => {
    if (busy()) return false;
    if (log) {
      const next = { ...layout(), log: { ...logRec(), visible: true } };
      commit(next);
      void save(next);
    }
    setBusy(true);
    setStatus("");
    try {
      await fn();
    } catch (e) {
      setStatus(String(e));
      setBusy(false);
      return false;
    }
    try {
      await refresh();
      setStatus(done);
    } catch (e) {
      setStatus(String(e));
    }
    setBusy(false);
    return true;
  };

  const openMenu = (e: MouseEvent, spec: { windowId?: string; sandboxId?: string } = {}) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu(menuHit(e.clientX, e.clientY, spec, layout().windows, sandboxes()));
  };

  const openOverlay = (next: Overlay) => setOverlay(next);

  return (
    <div
      class="relative h-full w-full bg-x11"
      onContextMenu={(e) => openMenu(e)}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) setMenu(null);
      }}
    >
      <Show when={ready()}>
        <IconManager
          layout={layout()}
          sandboxes={sandboxes()}
          hosts={hosts()}
          focus={focus()}
          live={live}
          busy={busy()}
          openMenu={openMenu}
          patchWin={patchWin}
          raise={raise}
          run={run}
          beginGeom={beginGeom}
          endGeom={endGeom}
          patchIcon={(patch) =>
            commit({
              ...layout(),
              icon_manager: { ...layout().icon_manager, ...patch },
              log: logRec(),
            })
          }
          hide={() =>
            void save({
              ...layout(),
              icon_manager: { ...layout().icon_manager, visible: false },
              log: logRec(),
            })
          }
        />
        <CanvasWindows
          layout={layout()}
          sandboxes={sandboxes()}
          hosts={hosts()}
          focus={focus()}
          live={live}
          busy={busy()}
          openMenu={openMenu}
          patchWin={patchWin}
          raise={raise}
          run={run}
          beginGeom={beginGeom}
          endGeom={endGeom}
          onEnvironment={(id) => setOverlay({ kind: "environment", id, ...placeOverlay() })}
        />
      </Show>
      <Show when={menu()}>
        {(hit) => (
          <AppMenu
            hit={hit()}
            sandbox={hit().sandbox ?? undefined}
            window={hit().window}
            iconMgr={layout().icon_manager.visible}
            layout={layout()}
            setOverlay={openOverlay}
            setMenu={setMenu}
            setStatus={setStatus}
            patchWin={patchWin}
            raise={raise}
            run={run}
            save={save}
            hosts={hosts()}
            onAttach={() => openOverlay({ kind: "attach", ...placeOverlay() })}
            onHosts={() => openOverlay({ kind: "hosts", ...placeOverlay() })}
          />
        )}
      </Show>
      <Show when={overlay()}>
        {(ov) => (
          <OverlayDialog
            overlay={ov()}
            sandbox={sandboxes().find((s) => overlayId(ov()) === s.id)}
            sandboxes={sandboxes()}
            hosts={hosts()}
            apiFor={(id) => clientFor(hosts(), id)}
            onAttach={(h) => {
              setRoster(upsertHost(hosts(), h));
              void refresh();
            }}
            onDetach={(id) => {
              setRoster(removeHost(hosts(), id));
              void refresh();
            }}
            move={(x, y) => setOverlay({ ...ov(), x, y })}
            pickSandbox={() => {
              setFocus(null);
              setOverlay(null);
            }}
            close={() => setOverlay(null)}
            busy={busy()}
            refresh={() => {
              void refresh();
            }}
            run={run}
          />
        )}
      </Show>
      <Show when={ready() && logRec().visible}>
        <LogWindow
          x={logRec().x}
          y={logRec().y}
          w={logRec().w}
          h={logRec().h}
          onMoveStart={beginGeom}
          onMove={(x, y) => commit({ ...layout(), log: { ...logRec(), x, y } })}
          onResize={(w, h) => commit({ ...layout(), log: { ...logRec(), w, h } })}
          onMoveEnd={endGeom}
          onClose={() => void save({ ...layout(), log: { ...logRec(), visible: false } })}
          hosts={hosts()}
          lines={logLines()}
          setLines={(lines) => {
            if (sameLines(logLines(), lines)) return;
            setLogLines(lines);
            writeCachedLogLines(lines);
          }}
        />
      </Show>
      <StatusLine busy={busy()} status={status()} />
    </div>
  );
}

function LogWindow(props: {
  x: number;
  y: number;
  w: number;
  h: number;
  onMoveStart: () => void;
  onMove: (x: number, y: number) => void;
  onResize: (w: number, h: number) => void;
  onMoveEnd: () => void;
  onClose: () => void;
  hosts: HostRec[];
  lines: string[];
  setLines: (lines: string[]) => void;
}) {
  onSettled(() => {
    const tick = () => {
      const roster = props.hosts;
      if (roster.length === 0) {
        api
          .progress()
          .then((r) => {
            if (!sameLines(props.lines, r.lines)) props.setLines(r.lines);
          })
          .catch((e) => {
            if (props.lines.length === 0) props.setLines([String(e)]);
          });
        return;
      }
      void Promise.all(
        roster.map(async (h) => {
          const r = await apiOn(hostScope(h.url, h.token)).progress();
          return r.lines.map((line) => (roster.length > 1 ? `${h.label}: ${line}` : line));
        }),
      )
        .then((chunks) => {
          const lines = chunks.flat();
          if (!sameLines(props.lines, lines)) props.setLines(lines);
        })
        .catch((e) => {
          if (props.lines.length === 0) props.setLines([String(e)]);
        });
    };
    tick();
    const t = setInterval(tick, 250);
    return () => clearInterval(t);
  });
  createEffect(
    () => props.lines,
    () => {
      const el = document.querySelector("[data-log]");
      if (el) el.scrollTop = el.scrollHeight;
    },
  );
  return (
    <Frame
      title="log"
      x={props.x}
      y={props.y}
      w={props.w}
      h={props.h}
      z={overlayZ}
      onMoveStart={props.onMoveStart}
      onMove={props.onMove}
      onResize={props.onResize}
      onMoveEnd={props.onMoveEnd}
      onClose={props.onClose}
    >
      <div
        data-log
        class="h-full overflow-auto bg-white p-2 font-mono text-[12px] leading-4 text-black"
      >
        <For
          each={props.lines}
          keyed={false}
          fallback={<div class="text-neutral-500">waiting…</div>}
        >
          {(line) => <div>{line()}</div>}
        </For>
      </div>
    </Frame>
  );
}

function StatusLine(props: { busy: boolean; status: string }) {
  return (
    <div
      class="pointer-events-none fixed bottom-2 left-2 px-2 py-0.5 font-mono text-xs font-bold"
      classList={{
        "bg-twm text-white": props.busy || props.status.length > 0,
        "text-black": !props.busy && props.status.length === 0,
      }}
    >
      {props.busy ? props.status || "working…" : props.status}
    </div>
  );
}

function IconManager(props: {
  layout: Layout;
  sandboxes: Sandbox[];
  hosts: HostRec[];
  focus: string | null;
  live: (id: string) => boolean;
  busy: boolean;
  openMenu: (e: MouseEvent, spec?: { windowId?: string; sandboxId?: string }) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
  beginGeom: () => void;
  endGeom: () => void;
  patchIcon: (patch: { x?: number; y?: number; w?: number; h?: number }) => void;
  hide: () => void;
}) {
  const im = () => props.layout.icon_manager;
  return (
    <Show when={im().visible}>
      <Frame
        title="Icon Manager"
        x={im().x}
        y={im().y}
        w={im().w}
        h={im().h}
        z={99990}
        onMoveStart={props.beginGeom}
        onMove={(x, y) => props.patchIcon({ x, y })}
        onResize={(w, h) => props.patchIcon({ w, h })}
        onMoveEnd={props.endGeom}
        onClose={props.hide}
        onContextMenu={(e) => props.openMenu(e)}
      >
        <div class="h-full overflow-auto bg-twm">
          <For each={props.layout.windows} keyed={(w) => w.id}>
            {(w) => (
              <button
                type="button"
                class="block w-full border-0 border-t border-twm-line bg-twm px-2 py-0.5 text-left font-bold text-white hover:bg-twm-hi"
                classList={{
                  "bg-twm-hi": props.focus === w().id,
                  "text-twm-muted": !props.live(w().sandbox),
                }}
                onClick={() => {
                  props.patchWin(w().id, { iconified: false }, true);
                  props.raise(w().id);
                  if (!props.live(w().sandbox) && !props.busy) {
                    props.run(
                      () =>
                        clientFor(props.hosts, w().host)
                          .start(w().sandbox)
                          .then(() => undefined),
                      "",
                      true,
                    );
                  }
                }}
                onContextMenu={(e) => props.openMenu(e, { windowId: w().id })}
              >
                {w().title}
              </button>
            )}
          </For>
          <For
            each={sandboxesWithoutWindows(props.sandboxes, props.layout.windows)}
            keyed={(s) => s.id}
          >
            {(s) => (
              <button
                type="button"
                class="block w-full border-0 border-t border-twm-line bg-twm px-2 py-0.5 text-left font-bold text-twm-muted hover:bg-twm-hi hover:text-white"
                onClick={() => {
                  if (props.busy) return;
                  props.run(
                    async () => {
                      const c = clientFor(props.hosts, s().host);
                      if (s().state !== "running") await c.start(s().id);
                      await c.openWindow(s().id);
                    },
                    "",
                    true,
                  );
                }}
                onContextMenu={(e) => props.openMenu(e, { sandboxId: s().id })}
              >
                {s().name} ({s().booting ? "starting" : s().state})
              </button>
            )}
          </For>
        </div>
      </Frame>
    </Show>
  );
}

function windowStatus(sandboxes: Sandbox[], id: string): string {
  const sb = sandboxes.find((s) => s.id === id);
  if (sb?.booting) return "starting…";
  return `stopped — Start ${sb?.name ?? "this Sandbox"}`;
}

function CanvasWindows(props: {
  layout: Layout;
  sandboxes: Sandbox[];
  hosts: HostRec[];
  focus: string | null;
  live: (id: string) => boolean;
  busy: boolean;
  openMenu: (e: MouseEvent, spec?: { windowId?: string; sandboxId?: string }) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
  beginGeom: () => void;
  endGeom: () => void;
  onEnvironment: (id: string) => void;
}) {
  return (
    <For each={props.layout.windows} keyed={(w) => w.id}>
      {(w) => (
        <Show when={!w().iconified}>
          <Frame
            title={w().title}
            x={w().x}
            y={w().y}
            w={w().w}
            h={w().h}
            z={w().z}
            dataWin={w().id}
            onMouseDown={() => {
              props.raise(w().id);
              if (!props.live(w().sandbox) && !props.busy) {
                props.run(
                  () =>
                    clientFor(props.hosts, w().host)
                      .start(w().sandbox)
                      .then(() => undefined),
                  "",
                  true,
                );
              }
            }}
            onContextMenu={(e) => props.openMenu(e, { windowId: w().id })}
            onMoveStart={props.beginGeom}
            onMove={(x, y) => props.patchWin(w().id, { x, y })}
            onMoveEnd={props.endGeom}
            onResize={(nw, nh) => props.patchWin(w().id, { w: nw, h: nh })}
            onIconify={() => props.patchWin(w().id, { iconified: true }, true)}
            onEnvironment={() => props.onEnvironment(w().sandbox)}
          >
            <Show
              when={props.live(w().sandbox)}
              fallback={
                <div class="flex h-full items-center justify-center font-twm text-[13px] text-neutral-500">
                  {windowStatus(props.sandboxes, w().sandbox)}
                </div>
              }
            >
              <Term
                windowId={w().id}
                host={hostById(props.hosts, w().host)}
                active={props.focus === w().id}
                onActivate={() => props.raise(w().id)}
              />
            </Show>
          </Frame>
        </Show>
      )}
    </For>
  );
}

function AppMenu(props: {
  hit: MenuHit;
  sandbox?: Sandbox;
  window: WindowRec | null;
  iconMgr: boolean;
  layout: Layout;
  setOverlay: (ov: Overlay) => void;
  setMenu: (m: MenuHit | null) => void;
  setStatus: (s: string) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
  save: (next: Layout) => void;
  onAttach: () => void;
  onHosts: () => void;
  hosts: HostRec[];
}) {
  const at = placeOverlay();
  const sb = () => props.sandbox;
  const winId = () => props.window?.id;
  const client = () => clientFor(props.hosts, sb()?.host);
  return (
    <RootMenu
      x={props.hit.x}
      y={props.hit.y}
      sandbox={props.sandbox}
      window={props.window}
      iconMgr={props.iconMgr}
      onNewSandbox={() => props.setOverlay({ kind: "sandbox", ...at })}
      onSandboxes={() => props.setOverlay({ kind: "sandboxes", ...at })}
      onSaveTemplate={() => {
        const box = sb();
        if (box) props.setOverlay({ kind: "save-template", id: box.id, ...at });
      }}
      onNewWindow={() => {
        const box = sb();
        if (!box || box.state !== "running") {
          props.setStatus("Start that Sandbox first");
          return;
        }
        props.run(async () => {
          await client().openWindow(box.id);
        });
      }}
      onIconify={() => {
        const id = winId();
        if (id) props.patchWin(id, { iconified: true }, true);
      }}
      onRaise={() => {
        const id = winId();
        if (id) props.raise(id);
      }}
      onLower={() => {
        const id = winId();
        if (id) props.patchWin(id, { z: 1 }, true);
      }}
      onCloseWindow={() => {
        const id = winId();
        if (id) props.run(() => client().closeWindow(id));
      }}
      onStart={() => {
        const box = sb();
        if (box)
          props.run(
            () =>
              client()
                .start(box.id)
                .then(() => undefined),
            "",
            true,
          );
      }}
      onStop={() => {
        const box = sb();
        if (box)
          props.run(() =>
            client()
              .stop(box.id)
              .then(() => undefined),
          );
      }}
      onDestroy={() => {
        const box = sb();
        if (box) props.setOverlay({ kind: "destroy", id: box.id, ...at });
      }}
      onReset={() => {
        const box = sb();
        if (box) props.setOverlay({ kind: "reset", id: box.id, ...at });
      }}
      onToggleIcons={() =>
        props.save({
          ...props.layout,
          icon_manager: {
            ...props.layout.icon_manager,
            visible: !props.layout.icon_manager.visible,
          },
        })
      }
      onLimits={() => {
        const box = sb();
        if (box) props.setOverlay({ kind: "limits", id: box.id, ...at });
      }}
      onEnvironment={() => {
        const box = sb();
        if (box) props.setOverlay({ kind: "environment", id: box.id, ...at });
      }}
      onTemplates={() => {
        props.setOverlay({ kind: "templates", ...at });
      }}
      onAttach={() => props.onAttach()}
      onHosts={() => props.onHosts()}
      close={() => props.setMenu(null)}
    />
  );
}
