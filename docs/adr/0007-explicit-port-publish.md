# Sandbox ports stay closed until Published

A workstation Agent will start preview servers. Leaving inbound closed forever makes those servers invisible to the Host browser. Auto-mapping every listener lets the Agent expose a proxy you did not ask for.

Publish is an explicit action: this Sandbox port appears on Host `127.0.0.1`. Default remains closed. Not bound on the LAN. SSH tunnels may still exist as a Unix side effect; they are not the feature.
