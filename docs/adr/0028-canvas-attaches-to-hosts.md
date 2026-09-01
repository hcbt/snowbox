# One person, many Hosts, one Canvas

ADR 0015 deferred opening the Daemon to other machines as a later product. That later product is one person, Hosts they own, one Canvas — not a multi-tenant cloud. A Host is a computer that runs a Daemon. The Canvas is a browser; it is not a Host. Attach (URL + token) is how this Canvas may call that Daemon. Opening the Canvas URL is not Attach. The roster lives in this browser. Loopback is still not auth. Unauthenticated loads do not receive the token except as a loopback convenience.

This supersedes the “opening it to other machines is a later product” clause of [0015](0015-documented-localhost-api.md). The API stays documented, versioned, and token-authenticated. Discovery never Attaches and does not list strangers.
