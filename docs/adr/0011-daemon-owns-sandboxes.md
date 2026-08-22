# The Daemon owns Sandboxes

The GUI is a localhost browser. If the UI process owns the VMs, closing a tab kills the Agent.

A Host Daemon owns Sandboxes and serves the UI. The browser is a client. Close the tab, the Agent keeps working. Quit the Daemon, Sandboxes stop. An always-on login item like Docker Desktop is later, not v1.

The Daemon is not an open port on localhost. Callers present a token from the user’s config directory. Loopback is not authentication.
