import type { JSX } from "solid-js";
import { Show } from "solid-js";

function CloseMark() {
  return (
    <svg width="8" height="8" viewBox="0 0 8 8" aria-hidden="true">
      <path
        d="M1.2 1.2 L6.8 6.8 M6.8 1.2 L1.2 6.8"
        fill="none"
        stroke="#fff"
        strokeWidth="1.4"
        strokeLinecap="square"
      />
    </svg>
  );
}

export function Frame(props: {
  title: string;
  x: number;
  y: number;
  z: number;
  w?: number;
  h?: number;
  dataWin?: string;
  sheen?: boolean;
  onMove: (x: number, y: number) => void;
  onMoveStart?: () => void;
  onMoveEnd?: () => void;
  onResize?: (w: number, h: number) => void;
  onIconify?: () => void;
  onEnvironment?: () => void;
  onClose?: () => void;
  onMouseDown?: () => void;
  onContextMenu?: (e: MouseEvent) => void;
  children: JSX.Element;
}) {
  let root: HTMLDivElement | undefined;
  const drag = (e: MouseEvent, kind: "move" | "resize" | "resize-e" | "resize-s") => {
    e.preventDefault();
    e.stopPropagation();
    props.onMouseDown?.();
    props.onMoveStart?.();
    const startX = e.clientX;
    const startY = e.clientY;
    const ox = props.x;
    const oy = props.y;
    const ow = props.w ?? 0;
    const oh = props.h ?? 0;
    const sized = (dx: number, dy: number) => ({
      w: Math.max(180, kind === "resize-s" ? ow : ow + dx),
      h: Math.max(80, kind === "resize-e" ? oh : oh + dy),
    });
    const move = (ev: MouseEvent) => {
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (!root) return;
      if (kind === "move") {
        root.style.transform = `translate(${dx}px, ${dy}px)`;
        return;
      }
      if (!props.onResize) return;
      const next = sized(dx, dy);
      if (kind !== "resize-s") root.style.width = `${next.w}px`;
      if (kind !== "resize-e") root.style.height = `${next.h}px`;
    };
    const up = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (kind === "move") {
        const nx = ox + dx;
        const ny = oy + dy;
        if (root) {
          root.style.left = `${nx}px`;
          root.style.top = `${ny}px`;
          root.style.transform = "";
        }
        props.onMove(nx, ny);
      } else if (props.onResize) {
        const next = sized(dx, dy);
        if (root) {
          root.style.width = `${next.w}px`;
          root.style.height = `${next.h}px`;
        }
        props.onResize(next.w, next.h);
      }
      props.onMoveEnd?.();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div
      ref={(el) => {
        root = el;
      }}
      class={`twm-float absolute flex min-h-[80px] min-w-[180px] flex-col${props.sheen ? " twm-sheen" : ""}`}
      data-win={props.dataWin}
      style={{
        left: `${props.x}px`,
        top: `${props.y}px`,
        width: props.w ? `${props.w}px` : undefined,
        height: props.h ? `${props.h}px` : undefined,
        "z-index": props.z,
      }}
      onMouseDown={() => props.onMouseDown?.()}
      onContextMenu={(e) => {
        if (!props.onContextMenu) return;
        e.preventDefault();
        e.stopPropagation();
        props.onContextMenu(e);
      }}
    >
      <div class="twm-bar cursor-grab active:cursor-grabbing" onMouseDown={(e) => drag(e, "move")}>
        <span class="twm-win-icon" />
        <span class="min-w-0 flex-1 overflow-hidden whitespace-nowrap">{props.title}</span>
        <Show when={props.onEnvironment}>
          <button
            type="button"
            class="twm-env"
            title="Environment"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              props.onEnvironment?.();
            }}
          >
            ENV
          </button>
        </Show>
        <Show when={props.onIconify}>
          <button
            type="button"
            class="twm-win-btn twm-iconify"
            title="Iconify"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              props.onIconify?.();
            }}
          />
        </Show>
        <Show when={props.onClose}>
          <button
            type="button"
            class="twm-win-btn"
            title="Close"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              props.onClose?.();
            }}
          >
            <CloseMark />
          </button>
        </Show>
      </div>
      <div class={`twm-body${props.sheen ? " twm-body-panel" : ""}`}>
        {props.children}
        <Show when={props.onResize && props.w !== undefined && props.h !== undefined}>
          <div
            class="absolute top-0 right-0 z-20 h-full w-2 cursor-e-resize"
            onMouseDown={(e) => drag(e, "resize-e")}
          />
          <div
            class="absolute bottom-0 left-0 z-20 h-2 w-full cursor-s-resize"
            onMouseDown={(e) => drag(e, "resize-s")}
          />
          <div
            class="absolute right-0 bottom-0 z-30 size-4 cursor-se-resize"
            onMouseDown={(e) => drag(e, "resize")}
          />
        </Show>
      </div>
    </div>
  );
}
