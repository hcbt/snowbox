import type { Sandbox, WindowRec } from "./api";
import type { Mode, Theme } from "./appearance";

function CheckItem(props: { checked: boolean; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      class="twm-menu-item"
      aria-checked={props.checked ? "true" : "false"}
      onClick={props.onClick}
    >
      <span class="inline-block w-4 shrink-0">{props.checked ? "✓" : ""}</span>
      {props.label}
    </button>
  );
}

export function RootMenu(props: {
  x: number;
  y: number;
  sandbox?: Sandbox;
  window: WindowRec | null;
  iconMgr: boolean;
  logVisible: boolean;
  theme: Theme;
  mode: Mode;
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
  onToggleLog: () => void;
  onTheme: (theme: Theme) => void;
  onMode: (mode: Mode) => void;
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
      class="twm-sheen twm-menu-float twm-menu absolute z-[100000]"
      style={{ left: `${props.x}px`, top: `${props.y}px` }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <div class="twm-menu-title">snowbox</div>
      <button type="button" class="twm-menu-item" onClick={() => go(props.onNewSandbox)}>
        New Sandbox
      </button>
      <button type="button" class="twm-menu-item" onClick={() => go(props.onSandboxes)}>
        Sandboxes…
      </button>
      <div class="twm-menu-rule" />
      <div class="twm-menu-head">{sb() ? `Sandbox ${sbName()}` : "no Sandbox selected"}</div>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb() || !running()}
        onClick={() => go(props.onNewWindow)}
      >
        New Window{sb() ? ` on ${sbName()}` : ""}
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb()}
        onClick={() => go(props.onSaveTemplate)}
      >
        Save {sb()?.name ?? "Sandbox"} as Template…
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb() || running()}
        onClick={() => go(props.onStart)}
      >
        Start {sbName()}
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb() || !running()}
        onClick={() => go(props.onStop)}
      >
        Stop {sbName()}
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb()}
        onClick={() => go(props.onDestroy)}
      >
        Destroy {sbName()}…
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb()}
        onClick={() => go(props.onReset)}
      >
        Reset {sbName()}…
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb()}
        onClick={() => go(props.onLimits)}
      >
        Limits of {sbName()}…
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!sb()}
        onClick={() => go(props.onEnvironment)}
      >
        Environment of {sbName()}…
      </button>
      <button type="button" class="twm-menu-item" onClick={() => go(props.onTemplates)}>
        Templates…
      </button>
      <button type="button" class="twm-menu-item" onClick={() => go(props.onAttach)}>
        Attach Host…
      </button>
      <button type="button" class="twm-menu-item" onClick={() => go(props.onHosts)}>
        Hosts…
      </button>
      <div class="twm-menu-rule" />
      <div class="twm-menu-head">{win() ? winTitle() : "no Window selected"}</div>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!win()}
        onClick={() => go(props.onIconify)}
      >
        Iconify {winTitle()}
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!win()}
        onClick={() => go(props.onRaise)}
      >
        Raise {winTitle()}
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!win()}
        onClick={() => go(props.onLower)}
      >
        Lower {winTitle()}
      </button>
      <button
        type="button"
        class="twm-menu-item"
        disabled={!win()}
        onClick={() => go(props.onCloseWindow)}
      >
        Close {winTitle()}
      </button>
      <div class="twm-menu-rule" />
      <div class="twm-menu-head">Theme</div>
      <CheckItem
        checked={props.theme === "twm"}
        label="twm"
        onClick={() => go(() => props.onTheme("twm"))}
      />
      <CheckItem
        checked={props.theme === "rio"}
        label="rio"
        onClick={() => go(() => props.onTheme("rio"))}
      />
      <div class="twm-menu-head">Mode</div>
      <CheckItem
        checked={props.mode === "night"}
        label="night"
        onClick={() => go(() => props.onMode("night"))}
      />
      <CheckItem
        checked={props.mode === "day"}
        label="day"
        onClick={() => go(() => props.onMode("day"))}
      />
      <div class="twm-menu-rule" />
      <button type="button" class="twm-menu-item" onClick={() => go(props.onToggleIcons)}>
        {props.iconMgr ? "Hide Icon Manager" : "Show Icon Manager"}
      </button>
      <button type="button" class="twm-menu-item" onClick={() => go(props.onToggleLog)}>
        {props.logVisible ? "Hide log" : "Show log"}
      </button>
    </div>
  );
}
