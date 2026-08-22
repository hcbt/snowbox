# Sandboxes have CPU, RAM, and disk Limits

Many concurrent Sandboxes, passwordless sudo, and no cap on processes will eat the Host if nothing throttles them. Isolation that only hides files is not enough.

Each Sandbox has CPU, RAM, and disk Limits, set in the UI at create and editable later, with boring defaults. This is not a platform quota system.
