import { Show, createEffect, createSignal, onSettled } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import type { HostRec } from "./hosts";
import { readTermPoster, writeTermPoster } from "./layout-sync";

type PtyFrame = string | ArrayBuffer | ArrayBufferView;

function isTextFrame(data: PtyFrame): data is string {
  return typeof data === "string";
}

function wsBytes(data: PtyFrame): string | Uint8Array {
  if (isTextFrame(data)) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  return "";
}

export function Term(props: {
  windowId: string;
  host?: HostRec;
  active?: boolean;
  onActivate?: () => void;
}) {
  let host: HTMLDivElement | undefined;
  let term: Terminal | undefined;
  const setHost = (el: HTMLDivElement) => {
    host = el;
  };
  const [poster, setPoster] = createSignal(readTermPoster(props.windowId));

  const grab = (e: Event) => {
    e.stopPropagation();
    props.onActivate?.();
    term?.focus();
  };

  onSettled(() => {
    const el = host;
    if (!el) return;
    const t = new Terminal({
      cols: 80,
      rows: 24,
      cursorBlink: true,
      fontFamily: "ui-monospace, Menlo, Monaco, monospace",
      fontSize: 13,
      scrollback: 5000,
      theme: {
        background: "#ffffff",
        foreground: "#000000",
        cursor: "#000000",
        selectionBackground: "#c3366d",
        selectionForeground: "#ffffff",
      },
    });
    term = t;
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(el);

    let dropped = false;
    let covering = poster() !== undefined;
    let ws: WebSocket | undefined;
    const enc = new TextEncoder();
    const uncover = () => {
      covering = false;
      setPoster(undefined);
    };
    const snap = () => {
      if (dropped || covering) return;
      const node = el.querySelector(".xterm");
      if (node) writeTermPoster(props.windowId, node.outerHTML);
    };
    const sendSize = () => {
      if (ws?.readyState === WebSocket.OPEN && t.cols > 0 && t.rows > 0) {
        ws.send(`resize ${t.cols} ${t.rows}`);
      }
    };
    t.onData((data) => {
      if (ws?.readyState === WebSocket.OPEN) ws.send(enc.encode(data));
    });
    t.onResize(() => sendSize());
    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(el);

    const base = props.host?.url ?? `${location.protocol}//${location.host}`;
    const token = props.host?.token;
    const wsUrl = new URL(`${base}/api/v1/windows/${props.windowId}/pty`);
    wsUrl.protocol = wsUrl.protocol === "https:" ? "wss:" : "ws:";
    if (token) wsUrl.searchParams.set("token", token);
    if (dropped) return;
    const sock = new WebSocket(wsUrl.toString());
    ws = sock;
    sock.binaryType = "arraybuffer";
    let got = false;
    const onOpen = () => {
      fit.fit();
      sendSize();
      if (props.active !== false) t.focus();
      window.setTimeout(() => {
        if (!dropped && !got) uncover();
      }, 100);
    };
    const onMessage = (ev: MessageEvent) => {
      got = true;
      // SAFETY: binaryType is arraybuffer; browsers send string or ArrayBuffer.
      const data = wsBytes(ev.data as PtyFrame);
      if (covering) {
        t.write(data, () => {
          uncover();
          snap();
        });
        return;
      }
      t.write(data);
    };
    const onClose = () => {
      if (!dropped) t.write("\r\n[window closed]\r\n");
    };
    const onError = () => {
      if (!dropped) t.write("\r\n[window: socket error]\r\n");
    };
    sock.addEventListener("open", onOpen);
    sock.addEventListener("message", onMessage);
    sock.addEventListener("close", onClose);
    sock.addEventListener("error", onError);

    requestAnimationFrame(() => {
      fit.fit();
      sendSize();
    });
    const tick = window.setInterval(snap, 1000);
    window.addEventListener("pagehide", snap);
    return () => {
      window.clearInterval(tick);
      window.removeEventListener("pagehide", snap);
      snap();
      dropped = true;
      term = undefined;
      ro.disconnect();
      sock.removeEventListener("open", onOpen);
      sock.removeEventListener("message", onMessage);
      sock.removeEventListener("close", onClose);
      sock.removeEventListener("error", onError);
      ws?.close();
      t.dispose();
    };
  });

  createEffect(
    () => props.active,
    (active) => {
      if (active) term?.focus();
    },
  );

  return (
    <div
      class="relative h-full w-full bg-white"
      data-win={props.windowId}
      onPointerDown={grab}
      onMouseDown={grab}
    >
      <div ref={setHost} class="h-full w-full" />
      <Show when={poster()}>
        {(html) => (
          <div
            class="pointer-events-none absolute inset-0 z-10 overflow-hidden bg-white"
            ref={(node) => {
              // SAFETY: html is .xterm outerHTML this Canvas stored for this Window.
              node.innerHTML = html();
            }}
          />
        )}
      </Show>
    </div>
  );
}
