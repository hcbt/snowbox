import { createEffect, onSettled } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

export function Term(props: {
  windowId: string;
  active?: boolean;
  onActivate?: () => void;
}) {
  let host!: HTMLDivElement;
  let term: Terminal | undefined;

  const grab = (e: Event) => {
    e.stopPropagation();
    props.onActivate?.();
    term?.focus();
  };

  onSettled(() => {
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
    t.open(host);

    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(
      `${proto}://${location.host}/api/v1/windows/${props.windowId}/pty`,
    );
    ws.binaryType = "arraybuffer";
    const sendSize = () => {
      if (ws.readyState === WebSocket.OPEN && t.cols > 0 && t.rows > 0) {
        ws.send(`resize ${t.cols} ${t.rows}`);
      }
    };
    ws.onopen = () => {
      fit.fit();
      sendSize();
      if (props.active !== false) t.focus();
    };
    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") {
        t.write(ev.data);
        return;
      }
      t.write(new Uint8Array(ev.data as ArrayBuffer));
    };
    const enc = new TextEncoder();
    t.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(enc.encode(data));
    });
    t.onResize(() => sendSize());
    let dropped = false;
    ws.onclose = () => {
      if (!dropped) t.write("\r\n[window closed]\r\n");
    };
    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(host);
    requestAnimationFrame(() => {
      fit.fit();
      sendSize();
    });
    return () => {
      dropped = true;
      term = undefined;
      ro.disconnect();
      ws.close();
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
      ref={host}
      data-win={props.windowId}
      class="h-full w-full bg-white"
      onPointerDown={grab}
      onMouseDown={grab}
    />
  );
}
