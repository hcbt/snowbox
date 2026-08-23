{
  description = "Snowbox: coding Agents in isolated Nix-built Linux Sandboxes";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      linux = "aarch64-linux";
      darwin = "aarch64-darwin";
      darwinPkgs = nixpkgs.legacyPackages.${darwin};
    in
    {
      nixosConfigurations.spike-a = nixpkgs.lib.nixosSystem {
        system = linux;
        modules = [ ./nix/guest-spike-a.nix ];
      };

      nixosConfigurations.spike-b = nixpkgs.lib.nixosSystem {
        system = linux;
        modules = [ ./nix/guest-spike-b.nix ];
      };

      packages.${linux}.hello = nixpkgs.legacyPackages.${linux}.hello;

      packages.${darwin} =
        let
          guestA = self.nixosConfigurations.spike-a;
          guestB = self.nixosConfigurations.spike-b;
          guestBoot =
            guest:
            let
              cfg = guest.config;
            in
            {
              kernel = "${cfg.system.build.kernel}/${cfg.system.boot.loader.kernelFile}";
              initrd = "${cfg.system.build.netbootRamdisk}/initrd";
              toplevel = "${cfg.system.build.toplevel}";
            };
          bootA = guestBoot guestA;
          bootB = guestBoot guestB;
          spike-a = darwinPkgs.callPackage ./nix/spike-a.nix bootA;
          spike-b = darwinPkgs.callPackage ./nix/spike-b.nix (
            bootB
            // {
              hello = "${self.packages.${linux}.hello}";
              spikeKey = "${./nix/spike-b/id_ed25519}";
            }
          );
        in
        {
          inherit spike-a spike-b;
          default = spike-a;
        };

      apps.${darwin} = {
        default = {
          type = "app";
          program = "${self.packages.${darwin}.spike-a}/bin/spike-a";
        };
        spike-a-prove = {
          type = "app";
          program = "${darwinPkgs.writeShellScript "spike-a-prove" ''
            exec ${self.packages.${darwin}.spike-a}/bin/spike-a prove
          ''}";
        };
        spike-b-prove = {
          type = "app";
          program = "${darwinPkgs.writeShellScript "spike-b-prove" ''
            exec ${self.packages.${darwin}.spike-b}/bin/spike-b prove
          ''}";
        };
      };
    };
}
