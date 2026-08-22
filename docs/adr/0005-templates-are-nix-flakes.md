# Templates are Nix flakes

The lobby still searches Packages by program name; Nix is not that UI. Authoring a Template is the hatch: there is no Snowbox-specific template format. A Template is a Nix flake. The GUI can still pick and save Templates; “save” writes a flake, and anyone who needs to can edit that flake.

Environment realization is therefore flake realization. The GUI noun remains Template, not Flake. A custom template schema would be a second package language beside nixpkgs, which we already chose as the catalog.
