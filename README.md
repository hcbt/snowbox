# snowbox

Run coding agents in isolated Linux sandboxes.

Each sandbox is a virtual machine with its own kernel, filesystem, and packages. The agent works there. Your machine is not its computer.

Copy a project in when the agent should see it. Publish a port when you want to open what it started. Reset restores the declared environment and leaves the project files in place.

snowbox is free software under the GNU General Public License v3. See [LICENSE](LICENSE).
