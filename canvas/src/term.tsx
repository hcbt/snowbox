import { onSettled } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

export function Term(props: { windowId: string }) {
  let host!: HTMLDivElement;

  onSettled(() => {
    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "ui-monospace, Menlo, Monaco, monospace",
      fontSize: 13,
      theme: {
        background: "#ffffff",
        foreground: "#000000",
        cursor: "#000000",
        selectionBackground: "#c3366d",
        selectionForeground: "#ffffff",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    requestAnimationFrame(() => fit.fit());

    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(
      `${proto}://${location.host}/api/v1/windows/${props.windowId}/pty`,
    );
    ws.binaryType = "arraybuffer";
    ws.onmessage = (ev) => {
      if (typeof ev.data === "string") {
        term.write(ev.data);
        return;
      }
      term.write(new Uint8Array(ev.data as ArrayBuffer));
    };
    term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(data);
    });
    let dropped = false;
    ws.onclose = () => {
      if (!dropped) term.write("\r\n[window closed]\r\n");
    };
    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(host);
    return () => {
      dropped = true;
      ro.disconnect();
      ws.close();
      term.dispose();
    };
  });

  return <div ref={host} class="h-full w-full bg-white" />;
}
