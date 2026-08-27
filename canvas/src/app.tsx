import { For, Show, createSignal, onSettled } from "solid-js";
import { Term } from "./term";
import { Frame } from "./frame";
import { RootMenu } from "./menu";
import { OverlayDialog } from "./dialogs";
import { placeOverlay, type MenuPos, type Overlay } from "./overlay";
import { api, type Layout, type Sandbox, type WindowRec } from "./api";
import { sandboxesWithoutWindows } from "./sandbox-icons";

function focusTerm(id: string): void {
  const el = document.querySelector(`[data-win="${id}"] textarea.xterm-helper-textarea`);
  if (el instanceof HTMLTextAreaElement) {
    el.focus({ preventScroll: true });
  }
}

function dragOffset(
  e: MouseEvent,
  origin: { x: number; y: number },
  onMove: (x: number, y: number) => void,
  onEnd: () => void,
): void {
  e.preventDefault();
  e.stopPropagation();
  const startX = e.clientX;
  const startY = e.clientY;
  const move = (ev: MouseEvent) => {
    onMove(origin.x + ev.clientX - startX, origin.y + ev.clientY - startY);
  };
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    onEnd();
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}

export function App() {
  const [sandboxes, setSandboxes] = createSignal<Sandbox[]>([]);
  const [layout, setLayout] = createSignal<Layout>({
    windows: [],
    icon_manager: { x: 8, y: 8, visible: true },
  });
  const [focus, setFocus] = createSignal<string | null>(null);
  const [picked, setPicked] = createSignal<string | null>(null);
  const [menu, setMenu] = createSignal<MenuPos | null>(null);
  const [overlay, setOverlay] = createSignal<Overlay | null>(null);
  const [status, setStatus] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [ready, setReady] = createSignal(false);

  const refresh = async () => {
    const [s, l] = await Promise.all([api.sandboxes(), api.layout()]);
    setSandboxes(s.sandboxes);
    const ids = new Set(s.sandboxes.map((b) => b.id));
    const windows = l.windows.filter((w) => ids.has(w.sandbox));
    const next = { ...l, windows };
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

  const focusedWindow = () => layout().windows.find((w) => w.id === focus()) ?? null;

  const targetSandbox = () => {
    const win = focusedWindow();
    if (win) return sandboxes().find((s) => s.id === win.sandbox);
    const id = picked();
    if (id) return sandboxes().find((s) => s.id === id);
    return undefined;
  };

  const raise = (id: string) => {
    setFocus(id);
    const win = layout().windows.find((w) => w.id === id);
    if (win) setPicked(win.sandbox);
    focusTerm(id);
    const z = Math.max(0, ...layout().windows.map((w) => w.z));
    const cur = layout().windows.find((w) => w.id === id);
    if (!cur || cur.z === z) return;
    patchWin(id, { z: z + 1 }, false);
    queueMicrotask(() => focusTerm(id));
  };

  const run = async (fn: () => Promise<void>, done = "") => {
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

  const openMenu = (e: MouseEvent, windowId?: string) => {
    e.preventDefault();
    e.stopPropagation();
    if (windowId) raise(windowId);
    setMenu({ x: e.clientX, y: e.clientY });
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
        pickSandbox={(id) => {
          setPicked(id);
          setFocus(null);
        }}
        patchWin={patchWin}
        raise={raise}
        run={run}
        drag={(e) =>
          dragOffset(
            e,
            layout().icon_manager,
            (x, y) =>
              setLayout({
                ...layout(),
                icon_manager: { ...layout().icon_manager, x, y },
              }),
            () => save(layout()),
          )
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
      />
      <Show when={menu()}>
        {(pos) => (
          <AppMenu
            pos={pos()}
            sandbox={targetSandbox()}
            window={focusedWindow()}
            iconMgr={layout().icon_manager.visible}
            layout={layout()}
            focus={focus()}
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
            pickSandbox={(id) => {
              setPicked(id);
              setFocus(null);
              setOverlay(null);
            }}
            close={() => setOverlay(null)}
            run={run}
          />
        )}
      </Show>
      <StatusLine busy={busy()} status={status()} />
    </div>
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
  openMenu: (e: MouseEvent, windowId?: string) => void;
  pickSandbox: (id: string) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>) => Promise<boolean>;
  drag: (e: MouseEvent) => void;
}) {
  return (
    <Show when={props.layout.icon_manager.visible}>
      <div
        class="absolute min-w-40 bg-twm"
        style={{
          left: `${props.layout.icon_manager.x}px`,
          top: `${props.layout.icon_manager.y}px`,
          "z-index": 99990,
        }}
        onContextMenu={(e) => props.openMenu(e)}
      >
        <div
          class="flex h-5 cursor-grab items-center gap-1.5 bg-twm px-[3px] text-[13px] font-bold text-white"
          onMouseDown={props.drag}
        >
          <span class="size-3 shrink-0 border border-white bg-twm-hi" />
          <span class="flex-1">Icon Manager</span>
        </div>
        <div class="border-x-2 border-b-2 border-twm">
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
                    props.run(() => api.start(w().sandbox));
                  }
                }}
                onContextMenu={(e) => props.openMenu(e, w().id)}
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
                  props.pickSandbox(s().id);
                  if (props.busy) return;
                  props.run(async () => {
                    if (s().state !== "running") await api.start(s().id);
                    await api.openWindow(s().id);
                  });
                }}
                onContextMenu={(e) => {
                  props.pickSandbox(s().id);
                  props.openMenu(e);
                }}
              >
                {s().name} ({s().state})
              </button>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
}

function CanvasWindows(props: {
  layout: Layout;
  sandboxes: Sandbox[];
  focus: string | null;
  live: (id: string) => boolean;
  busy: boolean;
  openMenu: (e: MouseEvent, windowId?: string) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>) => Promise<boolean>;
  save: () => void;
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
                props.run(() => api.start(w().sandbox));
              }
            }}
            onContextMenu={(e) => props.openMenu(e, w().id)}
            onMove={(x, y) => props.patchWin(w().id, { x, y })}
            onMoveEnd={props.save}
            onResize={(nw, nh) => props.patchWin(w().id, { w: nw, h: nh })}
            onIconify={() => props.patchWin(w().id, { iconified: true }, true)}
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
  pos: MenuPos;
  sandbox?: Sandbox;
  window: WindowRec | null;
  iconMgr: boolean;
  layout: Layout;
  focus: string | null;
  setOverlay: (ov: Overlay) => void;
  setMenu: (m: MenuPos | null) => void;
  setStatus: (s: string) => void;
  patchWin: (id: string, patch: Partial<WindowRec>, persist?: boolean) => void;
  raise: (id: string) => void;
  run: (fn: () => Promise<void>) => Promise<boolean>;
  save: (next: Layout) => void;
}) {
  const at = placeOverlay();
  const sb = () => props.sandbox;
  return (
    <RootMenu
      x={props.pos.x}
      y={props.pos.y}
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
        const id = props.focus;
        if (id) props.patchWin(id, { iconified: true }, true);
      }}
      onRaise={() => {
        const id = props.focus;
        if (id) props.raise(id);
      }}
      onLower={() => {
        const id = props.focus;
        if (id) props.patchWin(id, { z: 1 }, true);
      }}
      onCloseWindow={() => {
        const id = props.focus;
        if (id) props.run(() => api.closeWindow(id));
      }}
      onStart={() => {
        const box = sb();
        if (box) props.run(() => api.start(box.id).then(() => undefined));
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
      onHatch={() => {
        const box = sb();
        if (box) props.setOverlay({ kind: "hatch", id: box.id, ...at });
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
