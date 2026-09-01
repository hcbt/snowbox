import type { Sandbox, WindowRec } from "./api";

const item =
  "flex h-[26px] w-full cursor-pointer items-center border-0 bg-transparent px-3.5 text-left font-twm text-[13px] leading-4 font-medium text-white hover:bg-white/16 disabled:cursor-default disabled:text-twm-muted disabled:hover:bg-transparent";
const head =
  "flex h-6 items-center px-3.5 font-twm text-[11px] leading-[14px] font-medium tracking-[0.04em] text-twm-muted uppercase";

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
  onAttach: () => void;
  onHosts: () => void;
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
      class="twm-sheen twm-menu-float absolute z-[100000] flex w-[248px] flex-col text-white"
      style={{ left: `${props.x}px`, top: `${props.y}px` }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div class="flex h-7 items-center px-3.5 text-[13px] leading-4 font-medium tracking-[0.08em] text-white uppercase">
        snowbox
      </div>
      <button type="button" class={item} onClick={() => go(props.onNewSandbox)}>
        New Sandbox
      </button>
      <button type="button" class={item} onClick={() => go(props.onSandboxes)}>
        Sandboxes…
      </button>
      <div class="h-px bg-twm-line" />
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
      <button type="button" class={item} onClick={() => go(props.onAttach)}>
        Attach Host…
      </button>
      <button type="button" class={item} onClick={() => go(props.onHosts)}>
        Hosts…
      </button>
      <div class="h-px bg-twm-line" />
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
      <div class="h-px bg-twm-line" />
      <button type="button" class={item} onClick={() => go(props.onToggleIcons)}>
        {props.iconMgr ? "Hide Icon Manager" : "Show Icon Manager"}
      </button>
    </div>
  );
}
