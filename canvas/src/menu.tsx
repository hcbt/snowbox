import type { Sandbox, WindowRec } from "./api";

const item =
  "block w-full cursor-pointer border-0 bg-transparent px-3 py-0.5 text-left font-twm text-[13px] font-bold text-white hover:bg-twm-hi disabled:cursor-default disabled:text-twm-muted";
const head = "px-3 py-0.5 font-twm text-[11px] font-bold text-twm-muted";

export function RootMenu(props: {
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
  onEnvironment: () => void;
  onTemplates: () => void;
  onPublish: () => void;
  onCopy: (dir: "in" | "out") => void;
  close: () => void;
}) {
  const sb = () => props.sandbox;
  const win = () => props.window;
  const go = (fn: () => void) => {
    fn();
    props.close();
  };
  const sbName = () => sb()?.name ?? "";
  const winTitle = () => win()?.title ?? "";
  const running = () => sb()?.state === "running";
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
      <div class={head}>{sb() ? `Sandbox ${sbName()}` : "no Sandbox selected"}</div>
      <button
        type="button"
        class={item}
        disabled={!sb() || !running()}
        onClick={() => go(props.onNewWindow)}
      >
        New Window{sb() ? ` on ${sbName()}` : ""}
      </button>
      <button type="button" class={item} disabled={!sb()} onClick={() => go(props.onSaveTemplate)}>
        Save {sb()?.name ?? "Sandbox"} as Template…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || running()}
        onClick={() => go(props.onStart)}
      >
        Start {sbName()}
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || !running()}
        onClick={() => go(props.onStop)}
      >
        Stop {sbName()}
      </button>
      <button type="button" class={item} disabled={!sb()} onClick={() => go(props.onDestroy)}>
        Destroy {sbName()}…
      </button>
      <button type="button" class={item} disabled={!sb()} onClick={() => go(props.onReset)}>
        Reset {sbName()}…
      </button>
      <button type="button" class={item} disabled={!sb()} onClick={() => go(props.onLimits)}>
        Limits of {sbName()}…
      </button>
      <button type="button" class={item} disabled={!sb()} onClick={() => go(props.onEnvironment)}>
        Environment of {sbName()}…
      </button>
      <button type="button" class={item} onClick={() => go(props.onTemplates)}>
        Templates…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || !running()}
        onClick={() => go(props.onPublish)}
      >
        Publish {sbName()}…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || running()}
        onClick={() => go(() => props.onCopy("in"))}
      >
        Copy in to {sbName()}…
      </button>
      <button
        type="button"
        class={item}
        disabled={!sb() || running()}
        onClick={() => go(() => props.onCopy("out"))}
      >
        Copy out from {sbName()}…
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <div class={head}>{win() ? winTitle() : "no Window selected"}</div>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onIconify)}>
        Iconify {winTitle()}
      </button>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onRaise)}>
        Raise {winTitle()}
      </button>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onLower)}>
        Lower {winTitle()}
      </button>
      <button type="button" class={item} disabled={!win()} onClick={() => go(props.onCloseWindow)}>
        Close {winTitle()}
      </button>
      <div class="my-0.5 h-px bg-twm-line" />
      <button type="button" class={item} onClick={() => go(props.onToggleIcons)}>
        {props.iconMgr ? "Hide Icon Manager" : "Show Icon Manager"}
      </button>
    </div>
  );
}
