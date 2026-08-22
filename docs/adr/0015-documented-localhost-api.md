# The Daemon API is documented and local

v1 is not a platform, but other programs on the same Host (scripts, a second UI, a plugin) should be able to drive Sandboxes. An undocumentable private protocol makes that impossible. A network API is the platform.

The Daemon API is documented, versioned as a real contract, bound to `127.0.0.1`, and authenticated with the user token. The bundled UI is a client of that API. Breaking it is a decision, not a refactor. Opening it to other machines is a later product.
