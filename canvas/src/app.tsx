import { For, Show, createSignal, onSettled } from "solid-js";
import { Term } from "./term";
import { Frame } from "./frame";
import {
  api,
  type AgentProgram,
  type Layout,
  type Published,
  type Sandbox,
  type Template,
  type WindowRec,
} from "./api";

type Overlay =
  | null
  | { kind: "sandbox"; x: number; y: number }
  | { kind: "sandboxes"; x: number; y: number }
  | { kind: "limits"; id: string; x: number; y: number }
  | { kind: "hatch"; id: string; x: number; y: number }
  | { kind: "save-template"; id: string; x: number; y: number }
  | { kind: "publish"; id: string; x: number; y: number }
  | { kind: "copy"; id: string; dir: "in" | "out"; x: number; y: number }
  | { kind: "destroy"; id: string; x: number; y: number }
  | { kind: "reset"; id: string; x: number; y: number };

type Menu = { x: number; y: number } | null;

const overlayZ = 50000;

export function App() {
  const [sandboxes, setSandboxes] = createSignal<Sandbox[]>([]);
  const [layout, setLayout] = createSignal<Layout>({
    windows: [],
    icon_manager: { x: 8, y: 8, visible: true },
  });
  const [focus, setFocus] = createSignal<string | null>(null);
  const [picked, setPicked] = createSignal<string | null>(null);
  const [menu, setMenu] = createSignal<Menu>(null);
  const [overlay, setOverlay] = createSignal<Overlay>(null);
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
      windows: layout().windows.map((w) =>
        w.id === id ? { ...w, ...patch } : w,
      ),
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

  const focusTerm = (id: string) => {
    const el = document.querySelector(
      `[data-win="${id}"] textarea.xterm-helper-textarea`,
    ) as HTMLTextAreaElement | null;
    el?.focus({ preventScroll: true });
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

  const run = async (fn: () => Promise<unknown>, done = "") => {
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

  const placeOverlay = (x = 220, y = 56) => ({ x, y });

  const dragIcons = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startY = e.clientY;
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
  };

  return (
    <div
      class="relative h-full w-full bg-x11"
      onContextMenu={(e) => openMenu(e)}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) setMenu(null);
      }}
    >
      <Show when={layout().icon_manager.visible}>
        <div
          class="absolute min-w-40 bg-twm"
          style={{
            left: `${layout().icon_manager.x}px`,
            top: `${layout().icon_manager.y}px`,
            "z-index": 99990,
          }}
          onContextMenu={(e) => openMenu(e)}
        >
          <div
            class="flex h-5 cursor-grab items-center gap-1.5 bg-twm px-[3px] text-[13px] font-bold text-white"
            onMouseDown={dragIcons}
          >
            <span class="size-3 shrink-0 border border-white bg-twm-hi" />
            <span class="flex-1">Icon Manager</span>
          </div>
          <div class="border-x-2 border-b-2 border-twm">
            <For each={layout().windows} keyed={(w) => w.id}>
              {(w) => (
                <button
                  type="button"
                  class="block w-full border-0 border-t border-twm-line bg-twm px-2 py-0.5 text-left font-bold text-white hover:bg-twm-hi"
                  classList={{
                    "bg-twm-hi": focus() === w().id,
                    "text-twm-muted": !live(w().sandbox),
                  }}
                  onClick={() => {
                    patchWin(w().id, { iconified: false }, true);
                    raise(w().id);
                    if (!live(w().sandbox) && !busy()) run(() => api.start(w().sandbox));
                  }}
                  onContextMenu={(e) => openMenu(e, w().id)}
                >
                  {w().title}
                </button>
              )}
            </For>
          </div>
        </div>
      </Show>

      <For each={layout().windows} keyed={(w) => w.id}>
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
                raise(w().id);
                if (!live(w().sandbox) && !busy()) run(() => api.start(w().sandbox));
              }}
              onContextMenu={(e) => openMenu(e, w().id)}
              onMove={(x, y) => patchWin(w().id, { x, y })}
              onMoveEnd={() => save(layout())}
              onResize={(nw, nh) => patchWin(w().id, { w: nw, h: nh })}
              onIconify={() => patchWin(w().id, { iconified: true }, true)}
            >
              <Show
                when={live(w().sandbox)}
                fallback={
                  <div class="flex h-full items-center justify-center font-twm text-[13px] text-neutral-500">
                    stopped — Start {sandboxes().find((s) => s.id === w().sandbox)?.name ?? "this Sandbox"}
                  </div>
                }
              >
                <Term
                  windowId={w().id}
                  active={focus() === w().id}
                  onActivate={() => raise(w().id)}
                />
              </Show>
            </Frame>
          </Show>
        )}
      </For>

      <Show when={menu()}>
        <RootMenu
          x={menu()!.x}
          y={menu()!.y}
          sandbox={targetSandbox()}
          window={focusedWindow()}
          iconMgr={layout().icon_manager.visible}
          onNewSandbox={() => setOverlay({ kind: "sandbox", ...placeOverlay() })}
          onSandboxes={() => setOverlay({ kind: "sandboxes", ...placeOverlay() })}
          onSaveTemplate={() => {
            const sb = targetSandbox();
            if (sb) setOverlay({ kind: "save-template", id: sb.id, ...placeOverlay() });
          }}
          onNewWindow={() => {
            const sb = targetSandbox();
            if (!sb || sb.state !== "running") {
              setStatus("Start that Sandbox first");
              return;
            }
            run(async () => {
              await api.openWindow(sb.id);
            });
          }}
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
          onCloseWindow={() => {
            const id = focus();
            if (id) run(() => api.closeWindow(id));
          }}
          onStart={() => {
            const sb = targetSandbox();
            if (sb) run(() => api.start(sb.id));
          }}
          onStop={() => {
            const sb = targetSandbox();
            if (sb) run(() => api.stop(sb.id));
          }}
          onDestroy={() => {
            const sb = targetSandbox();
            if (sb) setOverlay({ kind: "destroy", id: sb.id, ...placeOverlay() });
          }}
          onReset={() => {
            const sb = targetSandbox();
            if (sb) setOverlay({ kind: "reset", id: sb.id, ...placeOverlay() });
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
            const sb = targetSandbox();
            if (sb) setOverlay({ kind: "limits", id: sb.id, ...placeOverlay() });
          }}
          onHatch={() => {
            const sb = targetSandbox();
            if (sb) setOverlay({ kind: "hatch", id: sb.id, ...placeOverlay() });
          }}
          onPublish={() => {
            const sb = targetSandbox();
            if (sb) setOverlay({ kind: "publish", id: sb.id, ...placeOverlay() });
          }}
          onCopy={(dir) => {
            const sb = targetSandbox();
            if (sb) setOverlay({ kind: "copy", id: sb.id, dir, ...placeOverlay() });
          }}
          close={() => setMenu(null)}
        />
      </Show>

      <Show when={overlay()}>
        <OverlayDialog
          overlay={overlay()!}
          sandbox={sandboxes().find((s) => {
            const ov = overlay();
            return ov !== null && "id" in ov && s.id === ov.id;
          })}
          sandboxes={sandboxes()}
          move={(x, y) => {
            const ov = overlay();
            if (ov) setOverlay({ ...ov, x, y });
          }}
          pickSandbox={(id) => {
            setPicked(id);
            setFocus(null);
            setOverlay(null);
          }}
          close={() => setOverlay(null)}
          run={run}
        />
      </Show>

      <div
        class="pointer-events-none fixed bottom-2 left-2 px-2 py-0.5 font-mono text-xs font-bold"
        classList={{
          "bg-twm text-white": busy() || status().length > 0,
          "text-black": !busy() && status().length === 0,
        }}
      >
        {busy() ? status() || "working…" : status()}
      </div>
    </div>
  );
}

function RootMenu(props: {
  x: number;
  y: number;
  sandbox?: Sandbox;
  window: WindowRec | null;
  iconMgr: boolean;
  onNewWindow: () => void;
  onNewSandbox: () => void;
  onSandboxes: () => void;
  onSaveTemplate: () => void;
  onIconify: () => void;
  onRaise: () => void;
  onLower: () => void;
  onCloseWindow: () => void;
  onStart: () => void;
  onStop: () => void;
  onDestroy: () => void;
  onReset: () => void;
  onToggleIcons: () => void;
  onLimits: () => void;
  onHatch: () => void;
  onPublish: () => void;
  onCopy: (dir: "in" | "out") => void;
  close: () => void;
}) {
  const item =
    "block w-full cursor-pointer border-0 bg-transparent px-3 py-0.5 text-left font-twm text-[13px] font-bold text-white hover:bg-twm-hi disabled:cursor-default disabled:text-twm-muted";
  const head = "px-3 py-0.5 font-twm text-[11px] font-bold text-twm-muted";
  const sb = () => props.sandbox;
  const win = () => props.window;
  const go = (fn: () => void) => {
    fn();
    props.close();
  };
  return (
    <div
      class="absolute z-[100000] min-w-52 border border-neutral-800 bg-twm text-white shadow-[1px_1px_0_#000]"
      style={{ left: `${props.x}px`, top: `${props.y}px` }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div class="bg-twm-head px-2.5 py-0.5 font-bold text-twm">snowbox</div>
      <button type="button" class={item} onClick={() => go(props.onNewSandbox)}>
        New Sandbox
      </button>
      <button type="button" class={item} onClick={() => go(props.onSandboxes)}>
        Sandboxes…
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <div class={head}>{sb() ? `Sandbox ${sb()!.name}` : "no Sandbox selected"}</div>
      <button
        type="button"
        class={item}
        disabled={!sb() || sb()!.state !== "running"}
        onClick={() => go(props.onNewWindow)}
      >
        New Window{sb() ? ` on ${sb()!.name}` : ""}
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb()}
        onClick={() => go(props.onSaveTemplate)}
      >
        Save {sb()?.name ?? "Sandbox"} as Template…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || sb()!.state === "running"}
        onClick={() => go(props.onStart)}
      >
        Start {sb()?.name ?? ""}
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || sb()!.state !== "running"}
        onClick={() => go(props.onStop)}
      >
        Stop {sb()?.name ?? ""}
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb()}
        onClick={() => go(props.onDestroy)}
      >
        Destroy {sb()?.name ?? ""}…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb()}
        onClick={() => go(props.onReset)}
      >
        Reset {sb()?.name ?? ""}…
      </button>
      <button type="button" class={item} disabled={!sb()} onClick={() => go(props.onLimits)}>
        Limits of {sb()?.name ?? ""}…
      </button>
      <button type="button" class={item} disabled={!sb()} onClick={() => go(props.onHatch)}>
        Hatch {sb()?.name ?? ""}…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || sb()!.state !== "running"}
        onClick={() => go(props.onPublish)}
      >
        Publish {sb()?.name ?? ""}…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || sb()!.state === "running"}
        onClick={() => go(() => props.onCopy("in"))}
      >
        Copy in to {sb()?.name ?? ""}…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || sb()!.state === "running"}
        onClick={() => go(() => props.onCopy("out"))}
      >
        Copy out from {sb()?.name ?? ""}…
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <div class={head}>{win() ? win()!.title : "no Window selected"}</div>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onIconify)}>
        Iconify {win()?.title ?? ""}
      </button>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onRaise)}>
        Raise {win()?.title ?? ""}
      </button>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onLower)}>
        Lower {win()?.title ?? ""}
      </button>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onCloseWindow)}>
        Close {win()?.title ?? ""}
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <button type="button" class={item} onClick={() => go(props.onToggleIcons)}>
        {props.iconMgr ? "Hide Icon Manager" : "Show Icon Manager"}
      </button>
    </div>
  );
}

function OverlayDialog(props: {
  overlay: NonNullable<Overlay>;
  sandbox?: Sandbox;
  sandboxes: Sandbox[];
  move: (x: number, y: number) => void;
  pickSandbox: (id: string) => void;
  close: () => void;
  run: (fn: () => Promise<unknown>, done?: string) => Promise<boolean>;
}) {
  const [name, setName] = createSignal("");
  const [cpu, setCpu] = createSignal(String(props.sandbox?.limits.cpu ?? 2));
  const [ram, setRam] = createSignal(
    String((props.sandbox?.limits.ram ?? 2147483648) / (1024 * 1024)),
  );
  const [disk, setDisk] = createSignal(
    String((props.sandbox?.limits.disk ?? 17179869184) / (1024 * 1024 * 1024)),
  );
  const [tpl, setTpl] = createSignal("empty");
  const [templates, setTemplates] = createSignal<Template[]>([]);
  const [agents, setAgents] = createSignal<AgentProgram[]>([]);
  const [envDoc, setEnvDoc] = createSignal<Record<string, unknown>>({});
  const [envCfg, setEnvCfg] = createSignal<Record<string, Record<string, unknown>>>({});
  const [path, setPath] = createSignal("");
  const [pubPort, setPubPort] = createSignal("3000");
  const [hostPort, setHostPort] = createSignal("");
  const [published, setPublished] = createSignal<Published[]>([]);
  const [replace, setReplace] = createSignal(false);

  onSettled(() => {
    if (props.overlay.kind === "sandbox" || props.overlay.kind === "save-template") {
      api.templates().then((r) => setTemplates(r.templates)).catch(() => {});
    }
    if (props.overlay.kind === "publish" && props.sandbox) {
      api.published(props.sandbox.id).then((r) => setPublished(r.published)).catch(() => {});
    }
    if (props.overlay.kind !== "hatch" || !props.sandbox) return;
    const id = props.sandbox.id;
    Promise.all([api.agentOptions(), api.environment(id)])
      .then(([opts, cfg]) => {
        setAgents(opts.programs);
        setEnvDoc(cfg);
        const programs = (cfg.programs ?? {}) as Record<string, Record<string, unknown>>;
        setEnvCfg(programs);
      })
      .catch(() => {});
  });

  const sbName = () => props.sandbox?.name ?? "";

  const title = () => {
    const ov = props.overlay;
    if (ov.kind === "sandbox") return "New Sandbox";
    if (ov.kind === "sandboxes") return "Sandboxes";
    if (ov.kind === "limits") return `Limits — ${sbName()}`;
    if (ov.kind === "hatch") return `Hatch — ${sbName()}`;
    if (ov.kind === "save-template") return `Save ${sbName()} as Template`;
    if (ov.kind === "publish") return `Publish — ${sbName()}`;
    if (ov.kind === "destroy") return `Destroy ${sbName()}`;
    if (ov.kind === "reset") return `Reset ${sbName()}`;
    if (ov.kind === "copy") return ov.dir === "in" ? `Copy in — ${sbName()}` : `Copy out — ${sbName()}`;
    return "snowbox";
  };

  const submit = async () => {
    const ov = props.overlay;
    if (ov.kind === "sandboxes") {
      props.close();
      return;
    }
    if (ov.kind === "save-template" && !name().trim()) return;

    let ok = false;
    if (ov.kind === "sandbox") {
      ok = await props.run(async () => {
        const sb = await api.create(name() || undefined, tpl() || undefined);
        await api.start(sb.id);
        await api.openWindow(sb.id);
      });
    } else if (ov.kind === "save-template") {
      ok = await props.run(() => api.saveTemplate(name().trim(), ov.id));
    } else if (ov.kind === "publish") {
      const host = hostPort().trim();
      await props.run(async () => {
        const pub = await api.publish(
          ov.id,
          Number(pubPort()),
          host ? Number(host) : undefined,
        );
        const list = await api.published(ov.id);
        const rows = list.published.some((p) => p.url === pub.url)
          ? list.published
          : [...list.published, pub];
        setPublished(rows);
      });
      return;
    } else if (ov.kind === "limits") {
      ok = await props.run(() =>
        api.patchLimits(ov.id, {
          cpu: Number(cpu()),
          ram: Number(ram()) * 1024 * 1024,
          disk: Number(disk()) * 1024 * 1024 * 1024,
        }),
      );
    } else if (ov.kind === "hatch") {
      const running = props.sandbox?.state === "running";
      ok = await props.run(
        () => api.saveEnvironment(ov.id, { ...envDoc(), programs: envCfg() }),
        running ? "" : "applies on Start",
      );
    } else if (ov.kind === "copy") {
      ok = await props.run(() =>
        ov.dir === "in"
          ? api.copyIn(ov.id, path(), replace())
          : api.copyOut(ov.id, path(), replace()),
      );
    } else if (ov.kind === "destroy") {
      ok = await props.run(() => api.destroy(ov.id));
    } else if (ov.kind === "reset") {
      ok = await props.run(() => api.reset(ov.id));
    }
    if (ok) props.close();
  };

  const field =
    "mt-0.5 w-full box-border border border-neutral-600 px-1 py-0.5 font-mono text-[13px]";
  const label = "mt-1.5 block font-bold";

  return (
    <Frame
      title={title()}
      x={props.overlay.x}
      y={props.overlay.y}
      z={overlayZ}
      onMove={props.move}
      onClose={props.close}
    >
      <form
        class="min-w-80 bg-white px-3.5 py-3 font-twm text-black"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <Show when={props.overlay.kind === "sandboxes"}>
          <div class="max-h-64 overflow-y-auto border border-neutral-400">
            <For
              each={props.sandboxes}
              keyed={(s) => s.id}
              fallback={<div class="px-2 py-1 text-[12px]">no Sandboxes</div>}
            >
              {(s) => (
                <button
                  type="button"
                  class="block w-full border-0 border-t border-neutral-300 bg-white px-2 py-1 text-left hover:bg-twm-hi hover:text-white"
                  onClick={() => props.pickSandbox(s().id)}
                >
                  <span class="font-bold">{s().name}</span>
                  <span class="ml-2 text-[12px]">{s().state}</span>
                </button>
              )}
            </For>
          </div>
        </Show>
        <Show when={props.overlay.kind === "destroy"}>
          <p class="text-[13px]">
            Destroy {sbName()}? Workspace is gone unless copy-out already happened.
          </p>
        </Show>
        <Show when={props.overlay.kind === "reset"}>
          <p class="text-[13px]">
            Reset {sbName()}? Declared Agents and devenv stay. Undeclared tools
            are gone. Workspace and Home remain.
          </p>
        </Show>
        <Show when={props.overlay.kind === "sandbox"}>
          <label class={label}>
            name
            <input class={field} value={name()} onInput={(e) => setName(e.currentTarget.value)} />
          </label>
          <label class={label}>
            template
            <select
              class={`${field} bg-white text-black`}
              value={tpl()}
              onChange={(e) => setTpl(e.currentTarget.value)}
            >
              <option value="empty">empty</option>
              <For
                each={templates().filter((t) => t.name !== "empty")}
                keyed={(t) => t.name}
              >
                {(t) => (
                  <option value={t().name}>
                    {t().name}
                    {t().shipped ? "" : " (saved)"}
                  </option>
                )}
              </For>
            </select>
          </label>
        </Show>
        <Show when={props.overlay.kind === "save-template"}>
          <label class={label}>
            name
            <input class={field} value={name()} onInput={(e) => setName(e.currentTarget.value)} />
          </label>
        </Show>
        <Show when={props.overlay.kind === "publish"}>
          <For each={published()} keyed={(p) => p.port}>
            {(p) => (
              <div class="flex items-center justify-between gap-2 text-[12px]">
                <a class="min-w-0 truncate text-twm" href={p().url}>
                  {p().url}
                </a>
                <span>:{p().port}</span>
                <button
                  type="button"
                  class="border border-twm-line bg-twm px-2 py-0.5 font-bold text-white"
                  onClick={() => {
                    const ov = props.overlay;
                    if (ov.kind !== "publish") return;
                    const port = p().port;
                    void props.run(async () => {
                      await api.unpublish(ov.id, port);
                      setPublished((await api.published(ov.id)).published);
                    });
                  }}
                >
                  Unpublish
                </button>
              </div>
            )}
          </For>
          <label class={label}>
            sandbox port
            <input class={field} value={pubPort()} onInput={(e) => setPubPort(e.currentTarget.value)} />
          </label>
          <label class={label}>
            host port (optional)
            <input class={field} value={hostPort()} onInput={(e) => setHostPort(e.currentTarget.value)} />
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
        <Show when={props.overlay.kind === "hatch"}>
          <p class="text-[12px]">devenv is always in the Environment. Secrets stay out of the flake.</p>
          <For
            each={agents()}
            keyed={(p) => p.name}
            fallback={<div class="text-[12px]">no Agents</div>}
          >
            {(p) => {
              const name = p().name;
              const cur = () => envCfg()[name] ?? {};
              const enabled = () => Boolean(cur().enable);
              const settings = () =>
                JSON.stringify(cur().settings ?? {}, null, 2);
              return (
                <div class="mt-2 border-t border-neutral-300 pt-2">
                  <label class="flex items-center gap-2 font-bold">
                    <input
                      type="checkbox"
                      checked={enabled()}
                      onChange={(e) => {
                        const on = e.currentTarget.checked;
                        setEnvCfg({
                          ...envCfg(),
                          [name]: { ...cur(), enable: on },
                        });
                      }}
                    />
                    {name}
                  </label>
                  <div class="text-[11px] text-neutral-600">{p().description}</div>
                  <label class={label}>
                    settings
                    <textarea
                      class={`${field} h-24`}
                      value={settings()}
                      onChange={(e) => {
                        try {
                          const parsed = JSON.parse(e.currentTarget.value || "{}");
                          setEnvCfg({
                            ...envCfg(),
                            [name]: { ...cur(), settings: parsed },
                          });
                        } catch {
                          /* keep typing */
                        }
                      }}
                    />
                  </label>
                </div>
              );
            }}
          </For>
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
        <Show when={props.overlay.kind !== "sandboxes"}>
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
              {props.overlay.kind === "destroy"
                ? `Destroy ${sbName()}`
                : props.overlay.kind === "reset"
                  ? `Reset ${sbName()}`
                  : "OK"}
            </button>
          </div>
        </Show>
      </form>
    </Frame>
  );
}
