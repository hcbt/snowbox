# The GUI is a Canvas of Windows

A list-plus-one-terminal UI makes many running Sandboxes invisible. The product is a Canvas: free-floating Windows, several Sandboxes at once, several Windows per Sandbox.

Each Window is a shell the Daemon owns, not `tmux` inside a single PTY. Closing a Window ends that shell; the Sandbox stays. Closing the browser does not forget Layout — the Daemon stores it on the Host. The Environment overlay, Templates, Limits, copy-in/out, and Publish are overlays on the Canvas, not a lobby. There is no Package catalog ([0024](0024-templates-are-home-manager-agent-config.md)). Extra Windows are not extra Agents ([0013](0013-agents-are-uncapped-per-sandbox.md)).
