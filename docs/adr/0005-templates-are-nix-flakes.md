# Templates are Nix flakes

Authoring a Template is the Canvas form (New Sandbox Customize, the Environment overlay, the Templates overlay): there is no Snowbox-specific template format. A Template is a Nix flake. The GUI can still pick and save Templates; “save” writes a flake, and anyone who needs to can edit that flake.

Environment realization is therefore flake realization. The GUI noun remains Template, not Flake. What the flake contains is devenv plus home-manager Agent config ([0024](0024-templates-are-home-manager-agent-config.md)).
