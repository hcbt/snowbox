import { For, Show, createEffect, createSignal, onSettled } from "solid-js";
import { Term } from "./term";
import { Frame } from "./frame";
import { RootMenu } from "./menu";
import { OverlayDialog } from "./dialogs";
import { overlayZ, placeOverlay, type Overlay } from "./overlay";
import { api, type Layout, type Sandbox, type WindowRec } from "./api";
import { sandboxesWithoutWindows } from "./sandbox-icons";
import { menuHit, type MenuHit } from "./menu-target";

function focusTerm(id: string): void {
  const el = document.querySelector(`[data-win="${id}"] textarea.xterm-helper-textarea`);
  if (el instanceof HTMLTextAreaElement) {
    el.focus({ preventScroll: true });
  }
}

export function App() {
  const [sandboxes, setSandboxes] = createSignal<Sandbox[]>([]);
  const [layout, setLayout] = createSignal<Layout>({
    windows: [],
    icon_manager: { x: 8, y: 8, w: 200, h: 240, visible: true },
  });
  const [focus, setFocus] = createSignal<string | null>(null);
  const [menu, setMenu] = createSignal<MenuHit | null>(null);
  const [overlay, setOverlay] = createSignal<Overlay | null>(null);
  const [status, setStatus] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [ready, setReady] = createSignal(false);
  const [logOpen, setLogOpen] = createSignal(false);
  const [logX, setLogX] = createSignal(240);
  const [logY, setLogY] = createSignal(72);
  const [logW, setLogW] = createSignal(560);
  const [logH, setLogH] = createSignal(280);

  const refresh = async () => {
    const [s, l] = await Promise.all([api.sandboxes(), api.layout()]);
    setSandboxes(s.sandboxes);
    const ids = new Set(s.sandboxes.map((b) => b.id));
    const windows = l.windows.filter((w) => ids.has(w.sandbox));
    const im = l.icon_manager;
    const next = {
      ...l,
      windows,
      icon_manager: {
        x: im.x,
        y: im.y,
        w: im.w > 0 ? im.w : 200,
        h: im.h > 0 ? im.h : 240,
        visible: im.visible,
      },
    };
    setLayout(next);
    setReady(true);
    if (windows.length !== l.windows.length) {
      await api.saveLayout(next);
    }
  };

  const live = (sandboxId: string) =>
    sandboxes().some((s) => s.id === sandboxId && s.state === "running");

  onSettled(() => {
    refresh().catch((e) => setStatus(String(e)));
    const t = setInterval(() => {
      refresh().catch(() => {});
    }, 2000);
    return () => clearInterval(t);
  });

  const save = async (next: Layout) => {
    if (!ready()) return;
    setLayout(next);
    try {
      setLayout(await api.saveLayout(next));
    } catch (e) {
      setStatus(String(e));
    }
  };

  const patchWin = (id: string, patch: Partial<WindowRec>, persist = false) => {
    const next: Layout = {
      ...layout(),
      windows: layout().windows.map((w) => (w.id === id ? { ...w, ...patch } : w)),
    };
    setLayout(next);
    if (persist) save(next);
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
    if (log) setLogOpen(true);
    setBusy(true);
    setStatus("");
    try {
      await fn();
      await refresh();
      setStatus(done);
      return true;
    } catch (e) {
      setStatus(String(e));
      return false;
    } finally {
      setBusy(false);
    }
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
      <IconManager
        layout={layout()}
        sandboxes={sandboxes()}
        focus={focus()}
        live={live}
        busy={busy()}
        openMenu={openMenu}
        patchWin={patchWin}
        raise={raise}
        run={run}
        patchIcon={(patch) =>
          setLayout({
            ...layout(),
            icon_manager: { ...layout().icon_manager, ...patch },
          })
        }
        saveIcon={() => save(layout())}
        hide={() =>
          save({
            ...layout(),
            icon_manager: { ...layout().icon_manager, visible: false },
          })
        }
      />
      <CanvasWindows
        layout={layout()}
        sandboxes={sandboxes()}
        focus={focus()}
        live={live}
        busy={busy()}
        openMenu={openMenu}
        patchWin={patchWin}
        raise={raise}
        run={run}
        save={() => save(layout())}
        onEnvironment={(id) => setOverlay({ kind: "environment", id, ...placeOverlay() })}
      />
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
          />
        )}
      </Show>
      <Show when={overlay()}>
        {(ov) => (
          <OverlayDialog
            overlay={ov()}
            sandbox={sandboxes().find((s) => "id" in ov() && s.id === ov().id)}
            sandboxes={sandboxes()}
            move={(x, y) => setOverlay({ ...ov(), x, y })}
            pickSandbox={() => {
              setFocus(null);
              setOverlay(null);
            }}
            close={() => setOverlay(null)}
            run={run}
          />
        )}
      </Show>
      <Show when={logOpen()}>
        <LogWindow
          x={logX()}
          y={logY()}
          w={logW()}
          h={logH()}
          onMove={(x, y) => {
            setLogX(x);
            setLogY(y);
          }}
          onResize={(w, h) => {
            setLogW(w);
            setLogH(h);
          }}
          onClose={() => setLogOpen(false)}
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
  onMove: (x: number, y: number) => void;
  onResize: (w: number, h: number) => void;
  onClose: () => void;
}) {
  const [lines, setLines] = createSignal<string[]>([]);
  onSettled(() => {
    const tick = () => {
      api
        .progress()
        .then((r) => setLines(r.lines))
        .catch((e) => {
          setLines((cur) => (cur.length > 0 ? cur : [String(e)]));
        });
    };
    tick();
    const t = setInterval(tick, 250);
    return () => clearInterval(t);
  });
  createEffect(
    () => lines(),
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
      onMove={props.onMove}
      onResize={props.onResize}
      onClose={props.onClose}
    >
      <div
        data-log
        class="h-full overflow-auto bg-white p-2 font-mono text-[12px] leading-4 text-black"
      >
        <For each={lines()} fallback={<div class="text-neutral-500">waiting…</div>}>
          {(line) => <div>{line}</div>}
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
  focus: string | null;
  live: (id: string) => boolean;
  busy: boolean;
  openMenu: (e: MouseEvent, spec?: { windowId?: string; sandboxId?: string }) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
  patchIcon: (patch: { x?: number; y?: number; w?: number; h?: number }) => void;
  saveIcon: () => void;
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
        onMove={(x, y) => props.patchIcon({ x, y })}
        onResize={(w, h) => props.patchIcon({ w, h })}
        onMoveEnd={props.saveIcon}
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
                    props.run(() => api.start(w().sandbox), "", true);
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
                      if (s().state !== "running") await api.start(s().id);
                      await api.openWindow(s().id);
                    },
                    "",
                    true,
                  );
                }}
                onContextMenu={(e) => props.openMenu(e, { sandboxId: s().id })}
              >
                {s().name} ({s().state})
              </button>
            )}
          </For>
        </div>
      </Frame>
    </Show>
  );
}

function CanvasWindows(props: {
  layout: Layout;
  sandboxes: Sandbox[];
  focus: string | null;
  live: (id: string) => boolean;
  busy: boolean;
  openMenu: (e: MouseEvent, spec?: { windowId?: string; sandboxId?: string }) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>, done?: string, log?: boolean) => Promise<boolean>;
  save: () => void;
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
                props.run(() => api.start(w().sandbox), "", true);
              }
            }}
            onContextMenu={(e) => props.openMenu(e, { windowId: w().id })}
            onMove={(x, y) => props.patchWin(w().id, { x, y })}
            onMoveEnd={props.save}
            onResize={(nw, nh) => props.patchWin(w().id, { w: nw, h: nh })}
            onIconify={() => props.patchWin(w().id, { iconified: true }, true)}
            onEnvironment={() => props.onEnvironment(w().sandbox)}
          >
            <Show
              when={props.live(w().sandbox)}
              fallback={
                <div class="flex h-full items-center justify-center font-twm text-[13px] text-neutral-500">
                  stopped — Start{" "}
                  {props.sandboxes.find((s) => s.id === w().sandbox)?.name ?? "this Sandbox"}
                </div>
              }
            >
              <Term
                windowId={w().id}
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
}) {
  const at = placeOverlay();
  const sb = () => props.sandbox;
  const winId = () => props.window?.id;
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
          await api.openWindow(box.id);
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
        if (id) props.run(() => api.closeWindow(id));
      }}
      onStart={() => {
        const box = sb();
        if (box) props.run(() => api.start(box.id).then(() => undefined), "", true);
      }}
      onStop={() => {
        const box = sb();
        if (box) props.run(() => api.stop(box.id).then(() => undefined));
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
      onPublish={() => {
        const box = sb();
        if (box) props.setOverlay({ kind: "publish", id: box.id, ...at });
      }}
      onCopy={(dir) => {
        const box = sb();
        if (box) props.setOverlay({ kind: "copy", id: box.id, dir, ...at });
      }}
      close={() => props.setMenu(null)}
    />
  );
}
