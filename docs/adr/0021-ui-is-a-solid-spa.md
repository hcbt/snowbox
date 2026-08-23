# The bundled UI is a Solid 2 SPA

The Canvas is a client-side window manager with live terminals. HTML from the Daemon (HTMX) cannot be that layer. A separate Node server would be a second process next to the Daemon.

The bundled UI is a TypeScript SPA: Solid 2, Vite, Tailwind. The Daemon serves the built files and the API ([0011](0011-daemon-owns-sandboxes.md), [0015](0015-documented-localhost-api.md)). Terminals are xterm.js; PTY I/O is a WebSocket to the Daemon, never to the guest. Overlays use unstyled primitives (Kobalte/corvu), not a SaaS kit. The UI tracks current releases of that stack — Solid 2, not 1.9 to avoid an RC. React matches diomedea and was rejected so this repo does not keep a second frontend brain by accident; the canvas-plus-xterm update model is why Solid won.
