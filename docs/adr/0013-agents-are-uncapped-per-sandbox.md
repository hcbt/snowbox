# A Sandbox may run any number of Agents

One Agent per box is the usual product. Two Agents on one Workspace will clobber files and share Home.

Snowbox does not cap that. A Sandbox may run none, one, or many Agents. They share Workspace, Home, and Environment. Conflict is accepted.

v1 does not orchestrate N Agents. Snowbox may start zero or one command. Anything else is a process in the shell. There is no vendor list (no first-class Grok/Claude/Codex); an Agent is a command in the Environment. Home does not pre-seed vendor auth dirs.
