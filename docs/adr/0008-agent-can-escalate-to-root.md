# The Agent starts unprivileged and can become root

The isolation claim is Host-vs-Sandbox, not a second user boundary inside the Sandbox. Starting the Agent as root makes the Package catalog and Reset optional: it can mutate the system at will. Forbidding root entirely fights “it is a Linux machine” (and the user’s own SSH/debug habits).

The Agent starts as an unprivileged user and has passwordless sudo. For the Agent the unprivileged default is theater; it still matters for a human who attaches. System mutations die on Reset. Persistence into Home only works on allowlisted paths. The Environment lives on the Host, so sudo inside the Sandbox cannot declare a Package.
