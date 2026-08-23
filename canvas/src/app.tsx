import { For, Show, createSignal, onSettled } from "solid-js";
import { Term } from "./term";
import {
  api,
  type Layout,
  type Sandbox,
  type WindowRec,
} from "./api";

type Overlay =
  | null
  | { kind: "sandbox" }
  | { kind: "limits"; id: string }
  | { kind: "packages"; id: string }
  | { kind: "copy"; id: string; dir: "in" | "out" };

type Menu = { x: number; y: number } | null;

export function App() {
  const [sandboxes, setSandboxes] = createSignal<Sandbox[]>([]);
  const [layout, setLayout] = createSignal<Layout>({
    windows: [],
    icon_manager: { x: 8, y: 8, visible: true },
  });
  const [focus, setFocus] = createSignal<string | null>(null);
  const [menu, setMenu] = createSignal<Menu>(null);
  const [overlay, setOverlay] = createSignal<Overlay>(null);
  const [status, setStatus] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [ready, setReady] = createSignal(false);

  const refresh = async () => {
    const [s, l] = await Promise.all([api.sandboxes(), api.layout()]);
    setSandboxes(s.sandboxes);
    setLayout(l);
    setReady(true);
  };

  onSettled(() => {
    refresh().catch((e) => setStatus(String(e)));
    const t = setInterval(() => {
      api.sandboxes().then((s) => setSandboxes(s.sandboxes)).catch(() => {});
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
      windows: layout().windows.map((w) =>
        w.id === id ? { ...w, ...patch } : w,
      ),
    };
    setLayout(next);
    if (persist) save(next);
  };

  const focusedSandbox = () => {
    const fid = focus();
    const win = layout().windows.find((w) => w.id === fid);
    if (win) return sandboxes().find((s) => s.id === win.sandbox);
    return sandboxes()[0];
  };

  const running = () => sandboxes().filter((s) => s.state === "running");

  const raise = (id: string) => {
    const z = Math.max(0, ...layout().windows.map((w) => w.z)) + 1;
    setFocus(id);
    patchWin(id, { z }, true);
  };

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setStatus("");
    try {
      await fn();
      await refresh();
    } catch (e) {
      setStatus(String(e));
    } finally {
      setBusy(false);
    }
  };

  const newWindow = () => {
    const sb =
      running().find((s) => s.id === focusedSandbox()?.id) ?? running()[0];
    if (!sb) {
      setStatus("start a Sandbox first");
      return;
    }
    run(async () => {
      await api.openWindow(sb.id);
    });
  };

  const drag = (
    e: MouseEvent,
    kind: "move" | "resize" | "icons",
    id?: string,
  ) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startY = e.clientY;
    if (kind === "icons") {
      const { x, y } = layout().icon_manager;
      const move = (ev: MouseEvent) => {
        setLayout({
          ...layout(),
          icon_manager: {
            ...layout().icon_manager,
            x: x + ev.clientX - startX,
            y: y + ev.clientY - startY,
          },
        });
      };
      const up = () => {
        window.removeEventListener("mousemove", move);
        window.removeEventListener("mouseup", up);
        save(layout());
      };
      window.addEventListener("mousemove", move);
      window.addEventListener("mouseup", up);
      return;
    }
    const win = layout().windows.find((w) => w.id === id);
    if (!win) return;
    raise(win.id);
    const { x, y, w, h } = win;
    const move = (ev: MouseEvent) => {
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (kind === "move") {
        patchWin(win.id, { x: x + dx, y: y + dy });
      } else {
        patchWin(win.id, {
          w: Math.max(180, w + dx),
          h: Math.max(80, h + dy),
        });
      }
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      save(layout());
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div
      class="relative h-full w-full bg-black"
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      onMouseDown={() => setMenu(null)}
    >
      <Show when={layout().icon_manager.visible}>
        <div
          class="absolute min-w-40 border-2 border-twm bg-twm"
          style={{
            left: `${layout().icon_manager.x}px`,
            top: `${layout().icon_manager.y}px`,
            "z-index": 99990,
          }}
        >
          <div
            class="flex h-5 cursor-grab items-center gap-1.5 bg-twm px-[3px] text-[13px] font-bold text-white"
            onMouseDown={(e) => drag(e, "icons")}
          >
            <span class="size-3 shrink-0 border border-white bg-twm-hi" />
            <span class="flex-1">Icon Manager</span>
          </div>
          <For each={layout().windows}>
            {(w) => (
              <button
                type="button"
                class="block w-full border-t border-twm-line px-2 py-0.5 text-left font-bold text-white hover:bg-twm-hi"
                classList={{ "bg-twm-hi": focus() === w.id }}
                onClick={() => {
                  patchWin(w.id, { iconified: false }, true);
                  raise(w.id);
                }}
              >
                {w.title}
              </button>
            )}
          </For>
        </div>
      </Show>

      <For each={layout().windows}>
        {(w) => (
          <Show when={!w.iconified}>
            <div
              class="absolute box-border flex min-h-20 min-w-[180px] flex-col border-2 border-twm bg-twm"
              style={{
                left: `${w.x}px`,
                top: `${w.y}px`,
                width: `${w.w}px`,
                height: `${w.h}px`,
                "z-index": w.z,
              }}
              onMouseDown={() => raise(w.id)}
            >
              <div
                class="flex h-5 cursor-grab items-center gap-1.5 bg-twm px-[3px] text-[13px] font-bold text-white active:cursor-grabbing"
                onMouseDown={(e) => drag(e, "move", w.id)}
              >
                <span class="relative size-3 shrink-0 border border-white bg-twm-hi">
                  <span class="absolute top-[3px] left-[3px] size-1.5 bg-white" />
                </span>
                <span class="flex-1 overflow-hidden whitespace-nowrap">
                  {w.title}
                </span>
                <button
                  type="button"
                  class="relative size-3.5 shrink-0 border border-white bg-twm"
                  title="Iconify"
                  onClick={(e) => {
                    e.stopPropagation();
                    patchWin(w.id, { iconified: true }, true);
                  }}
                >
                  <span class="absolute right-0.5 bottom-[3px] left-0.5 h-0.5 bg-white" />
                </button>
              </div>
              <div class="relative min-h-0 flex-1 bg-white">
                <Term windowId={w.id} />
                <div
                  class="absolute right-0 bottom-0 size-3 cursor-se-resize"
                  onMouseDown={(e) => drag(e, "resize", w.id)}
                />
              </div>
            </div>
          </Show>
        )}
      </For>

      <Show when={menu()}>
        <RootMenu
          x={menu()!.x}
          y={menu()!.y}
          sandbox={focusedSandbox()}
          hasRunning={running().length > 0}
          iconMgr={layout().icon_manager.visible}
          onNewWindow={newWindow}
          onNewSandbox={() => setOverlay({ kind: "sandbox" })}
          onIconify={() => {
            const id = focus();
            if (id) patchWin(id, { iconified: true }, true);
          }}
          onRaise={() => {
            const id = focus();
            if (id) raise(id);
          }}
          onLower={() => {
            const id = focus();
            if (id) patchWin(id, { z: 1 }, true);
          }}
          onKill={() => {
            const id = focus();
            if (id) run(() => api.closeWindow(id));
          }}
          onStart={() => {
            const sb = focusedSandbox();
            if (sb) run(() => api.start(sb.id));
          }}
          onStop={() => {
            const sb = focusedSandbox();
            if (sb) run(() => api.stop(sb.id));
          }}
          onDestroy={() => {
            const sb = focusedSandbox();
            if (sb) run(() => api.destroy(sb.id));
          }}
          onToggleIcons={() =>
            save({
              ...layout(),
              icon_manager: {
                ...layout().icon_manager,
                visible: !layout().icon_manager.visible,
              },
            })
          }
          onLimits={() => {
            const sb = focusedSandbox();
            if (sb) setOverlay({ kind: "limits", id: sb.id });
          }}
          onPackages={() => {
            const sb = focusedSandbox();
            if (sb) setOverlay({ kind: "packages", id: sb.id });
          }}
          onCopy={(dir) => {
            const sb = focusedSandbox();
            if (sb) setOverlay({ kind: "copy", id: sb.id, dir });
          }}
          close={() => setMenu(null)}
        />
      </Show>

      <Show when={overlay()}>
        <OverlayDialog
          overlay={overlay()!}
          sandbox={sandboxes().find((s) => {
            const ov = overlay();
            return ov !== null && ov.kind !== "sandbox" && s.id === ov.id;
          })}
          close={() => setOverlay(null)}
          run={run}
        />
      </Show>

      <div
        class="pointer-events-none fixed bottom-2 left-2 font-mono text-xs text-twm-hi"
        classList={{ "text-white": busy() }}
      >
        {busy() ? "working…" : status()}
      </div>
    </div>
  );
}

function RootMenu(props: {
  x: number;
  y: number;
  sandbox?: Sandbox;
  hasRunning: boolean;
  iconMgr: boolean;
  onNewWindow: () => void;
  onNewSandbox: () => void;
  onIconify: () => void;
  onRaise: () => void;
  onLower: () => void;
  onKill: () => void;
  onStart: () => void;
  onStop: () => void;
  onDestroy: () => void;
  onToggleIcons: () => void;
  onLimits: () => void;
  onPackages: () => void;
  onCopy: (dir: "in" | "out") => void;
  close: () => void;
}) {
  const item =
    "block w-full cursor-pointer border-0 bg-transparent px-3 py-0.5 text-left font-twm text-[13px] font-bold text-white hover:bg-twm-hi disabled:cursor-default disabled:text-twm-muted";
  return (
    <div
      class="absolute z-[100000] min-w-40 border border-neutral-800 bg-twm text-white shadow-[1px_1px_0_#000]"
      style={{ left: `${props.x}px`, top: `${props.y}px` }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div class="bg-twm-head px-2.5 py-0.5 font-bold text-twm">snowbox</div>
      <button type="button" class={item} onClick={() => { props.onNewSandbox(); props.close(); }}>
        New Sandbox
      </button>
      <button
        type="button"
        class={item}
        disabled={!props.hasRunning}
        onClick={() => { props.onNewWindow(); props.close(); }}
      >
        New Window
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <button type="button" class={item} onClick={() => { props.onIconify(); props.close(); }}>
        Iconify
      </button>
      <button type="button" class={item} onClick={() => { props.onRaise(); props.close(); }}>
        Raise
      </button>
      <button type="button" class={item} onClick={() => { props.onLower(); props.close(); }}>
        Lower
      </button>
      <button type="button" class={item} onClick={() => { props.onToggleIcons(); props.close(); }}>
        {props.iconMgr ? "Hide Iconmgr" : "Show Iconmgr"}
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <button
        type="button"
        class={item}
        disabled={!props.sandbox || props.sandbox.state === "running"}
        onClick={() => { props.onStart(); props.close(); }}
      >
        Start
      </button>
      <button
        type="button"
        class={item}
        disabled={!props.sandbox || props.sandbox.state !== "running"}
        onClick={() => { props.onStop(); props.close(); }}
      >
        Stop
      </button>
      <button
        type="button"
        class={item}
        disabled={!props.sandbox}
        onClick={() => { props.onDestroy(); props.close(); }}
      >
        Destroy
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <button type="button" class={item} disabled={!props.sandbox} onClick={() => { props.onLimits(); props.close(); }}>
        Limits…
      </button>
      <button type="button" class={item} disabled={!props.sandbox} onClick={() => { props.onPackages(); props.close(); }}>
        Packages…
      </button>
      <button type="button" class={item} disabled={!props.sandbox} onClick={() => { props.onCopy("in"); props.close(); }}>
        Copy in…
      </button>
      <button type="button" class={item} disabled={!props.sandbox} onClick={() => { props.onCopy("out"); props.close(); }}>
        Copy out…
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <button type="button" class={item} onClick={() => { props.onKill(); props.close(); }}>
        Kill
      </button>
    </div>
  );
}

function OverlayDialog(props: {
  overlay: NonNullable<Overlay>;
  sandbox?: Sandbox;
  close: () => void;
  run: (fn: () => Promise<unknown>) => void;
}) {
  const [name, setName] = createSignal("");
  const [cpu, setCpu] = createSignal(String(props.sandbox?.limits.cpu ?? 2));
  const [ram, setRam] = createSignal(
    String((props.sandbox?.limits.ram ?? 2147483648) / (1024 * 1024)),
  );
  const [disk, setDisk] = createSignal(
    String((props.sandbox?.limits.disk ?? 17179869184) / (1024 * 1024 * 1024)),
  );
  const [pkg, setPkg] = createSignal("");
  const [path, setPath] = createSignal("");
  const [replace, setReplace] = createSignal(false);
  const title =
    props.overlay.kind === "sandbox"
      ? "New Sandbox"
      : props.overlay.kind === "limits"
        ? "Limits"
        : props.overlay.kind === "packages"
          ? "Packages"
          : props.overlay.kind === "copy" && props.overlay.dir === "in"
            ? "Copy in"
            : "Copy out";

  const submit = () => {
    const ov = props.overlay;
    if (ov.kind === "sandbox") {
      props.run(() => api.create(name() || undefined));
    } else if (ov.kind === "limits") {
      props.run(() =>
        api.patchLimits(ov.id, {
          cpu: Number(cpu()),
          ram: Number(ram()) * 1024 * 1024,
          disk: Number(disk()) * 1024 * 1024 * 1024,
        }),
      );
    } else if (ov.kind === "packages") {
      props.run(() => api.addPackage(ov.id, pkg()));
    } else if (ov.kind === "copy") {
      props.run(() =>
        ov.dir === "in"
          ? api.copyIn(ov.id, path(), replace())
          : api.copyOut(ov.id, path(), replace()),
      );
    }
    props.close();
  };

  const field =
    "mt-0.5 w-full box-border border border-neutral-600 px-1 py-0.5 font-mono text-[13px]";
  const label = "mt-1.5 block font-bold";

  return (
    <div
      class="absolute top-16 left-1/2 z-[99999] min-w-80 -translate-x-1/2 border-2 border-twm bg-white text-black"
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div class="flex h-5 items-center gap-1.5 bg-twm px-[3px] text-[13px] font-bold text-white">
        <span class="size-3 shrink-0 border border-white bg-twm-hi" />
        <span class="flex-1">{title}</span>
        <button type="button" class="px-1 text-white" onClick={props.close}>
          ×
        </button>
      </div>
      <form
        class="px-3.5 py-3 font-twm"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <Show when={props.overlay.kind === "sandbox"}>
          <label class={label}>
            name
            <input class={field} value={name()} onInput={(e) => setName(e.currentTarget.value)} />
          </label>
        </Show>
        <Show when={props.overlay.kind === "limits"}>
          <label class={label}>
            cpu
            <input class={field} value={cpu()} onInput={(e) => setCpu(e.currentTarget.value)} />
          </label>
          <label class={label}>
            ram (MiB)
            <input class={field} value={ram()} onInput={(e) => setRam(e.currentTarget.value)} />
          </label>
          <label class={label}>
            disk (GiB)
            <input class={field} value={disk()} onInput={(e) => setDisk(e.currentTarget.value)} />
          </label>
        </Show>
        <Show when={props.overlay.kind === "packages"}>
          <label class={label}>
            add package
            <input class={field} value={pkg()} onInput={(e) => setPkg(e.currentTarget.value)} />
          </label>
        </Show>
        <Show when={props.overlay.kind === "copy"}>
          <label class={label}>
            host path
            <input class={field} value={path()} onInput={(e) => setPath(e.currentTarget.value)} />
          </label>
          <label class="mt-2 flex items-center gap-2 font-bold">
            <input
              type="checkbox"
              checked={replace()}
              onChange={(e) => setReplace(e.currentTarget.checked)}
            />
            replace
          </label>
        </Show>
        <div class="mt-3 flex justify-end gap-2">
          <button
            type="button"
            class="border border-twm-line bg-twm px-3 py-0.5 font-bold text-white"
            onClick={props.close}
          >
            Cancel
          </button>
          <button
            type="submit"
            class="border border-twm-line bg-twm px-3 py-0.5 font-bold text-white"
          >
            OK
          </button>
        </div>
      </form>
    </div>
  );
}
