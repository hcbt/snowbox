import type { JSX } from "solid-js";
import { Show } from "solid-js";

export function Frame(props: {
  title: string;
  x: number;
  y: number;
  z: number;
  w?: number;
  h?: number;
  dataWin?: string;
  onMove: (x: number, y: number) => void;
  onMoveEnd?: () => void;
  onResize?: (w: number, h: number) => void;
  onIconify?: () => void;
  onEnvironment?: () => void;
  onClose?: () => void;
  onMouseDown?: () => void;
  onContextMenu?: (e: MouseEvent) => void;
  children: JSX.Element;
}) {
  const drag = (e: MouseEvent, kind: "move" | "resize" | "resize-e" | "resize-s") => {
    e.preventDefault();
    e.stopPropagation();
    props.onMouseDown?.();
    const startX = e.clientX;
    const startY = e.clientY;
    const ox = props.x;
    const oy = props.y;
    const ow = props.w ?? 0;
    const oh = props.h ?? 0;
    const move = (ev: MouseEvent) => {
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (kind === "move") {
        props.onMove(ox + dx, oy + dy);
        return;
      }
      if (!props.onResize) return;
      props.onResize(
        Math.max(180, kind === "resize-s" ? ow : ow + dx),
        Math.max(80, kind === "resize-e" ? oh : oh + dy),
      );
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      props.onMoveEnd?.();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div
      class="absolute flex min-h-20 min-w-[180px] flex-col"
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
      <div
        class="flex h-5 shrink-0 cursor-grab items-center gap-1 bg-twm px-[3px] text-[13px] font-bold text-white active:cursor-grabbing"
        onMouseDown={(e) => drag(e, "move")}
      >
        <span class="relative size-3 shrink-0 border border-white bg-twm-hi">
          <span class="absolute top-[3px] left-[3px] size-1.5 bg-white" />
        </span>
        <span class="min-w-0 flex-1 overflow-hidden whitespace-nowrap">{props.title}</span>
        <Show when={props.onEnvironment}>
          <button
            type="button"
            class="shrink-0 border border-white px-0.5 text-[10px] leading-none text-white"
            title="Environment"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              props.onEnvironment?.();
            }}
          >
            env
          </button>
        </Show>
        <Show when={props.onIconify}>
          <button
            type="button"
            class="relative size-3 shrink-0 border border-white bg-twm"
            title="Iconify"
            onClick={(e) => {
              e.stopPropagation();
              props.onIconify?.();
            }}
          >
            <span class="absolute right-[2px] bottom-[3px] left-[2px] h-0.5 bg-white" />
          </button>
        </Show>
        <Show when={props.onClose}>
          <button
            type="button"
            class="flex size-3 shrink-0 items-center justify-center border border-white text-[11px] leading-none text-white"
            title="Close"
            onClick={(e) => {
              e.stopPropagation();
              props.onClose?.();
            }}
          >
            ×
          </button>
        </Show>
      </div>
      <div class="relative min-h-0 flex-1 border-x-2 border-b-2 border-twm bg-white">
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
