# snowbox

Run coding agents in isolated Linux sandboxes.

Each sandbox is a virtual machine with its own kernel, filesystem, and packages. The agent works there. Your machine is not its computer.

Clone the project inside the Sandbox; the Host has no live view of `/workspace`. Reset restores the declared environment and leaves the project files in place. One Canvas may Attach several Hosts.

snowbox is free software under the GNU General Public License v3. See [LICENSE](LICENSE).
