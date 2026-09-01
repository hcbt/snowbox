# The Daemon API is documented and token-authenticated

v1 is not a platform, but other programs (scripts, a second UI, a plugin) should be able to drive Sandboxes. An undocumentable private protocol makes that impossible. A network API without a token is the platform.

The Daemon API is documented, versioned as a real contract, and authenticated with the user token. The bundled UI is a client of that API. Breaking it is a decision, not a refactor. Callers may be on this Host or a Canvas the person Attached ([0028](0028-canvas-attaches-to-hosts.md)). Loopback is not authentication.

Supersedes the loopback-only / “later product” clause; see [0028](0028-canvas-attaches-to-hosts.md).
