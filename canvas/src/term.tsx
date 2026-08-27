import { createEffect, onSettled } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { json } from "./api";

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

export function Term(props: { windowId: string; active?: boolean; onActivate?: () => void }) {
  let host: HTMLDivElement | undefined;
  let term: Terminal | undefined;
  const setHost = (el: HTMLDivElement) => {
    host = el;
  };

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
    let ws: WebSocket | undefined;
    const enc = new TextEncoder();
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

    const proto = location.protocol === "https:" ? "wss" : "ws";
    // Mint the session cookie (browsers cannot set Authorization on
    // WebSocket) then upgrade with Origin + cookie.
    void json("/api/v1/health")
      .then(() => {
        if (dropped) return;
        const sock = new WebSocket(
          `${proto}://${location.host}/api/v1/windows/${props.windowId}/pty`,
        );
        ws = sock;
        sock.binaryType = "arraybuffer";
        sock.onopen = () => {
          fit.fit();
          sendSize();
          if (props.active !== false) t.focus();
        };
        sock.onmessage = (ev) => {
          // SAFETY: binaryType is arraybuffer; browsers send string or ArrayBuffer.
          t.write(wsBytes(ev.data as PtyFrame));
        };
        sock.onclose = () => {
          if (!dropped) t.write("\r\n[window closed]\r\n");
        };
      })
      .catch((e) => {
        if (!dropped) t.write(`\r\n[window: ${e}]\r\n`);
      });

    requestAnimationFrame(() => {
      fit.fit();
      sendSize();
    });
    return () => {
      dropped = true;
      term = undefined;
      ro.disconnect();
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
      ref={setHost}
      data-win={props.windowId}
      class="h-full w-full bg-white"
      onPointerDown={grab}
      onMouseDown={grab}
    />
  );
}
