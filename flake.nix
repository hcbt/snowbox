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
          hello = self.packages.${linux}.hello;
          mkSpike =
            {
              name,
              guest,
              script,
              extraExports ? "",
              extraInputs ? [ ],
            }:
            darwinPkgs.writeShellApplication {
              inherit name;
              runtimeInputs = [
                darwinPkgs.vfkit
                darwinPkgs.coreutils
                darwinPkgs.curl
                darwinPkgs.gnugrep
                darwinPkgs.gnused
                darwinPkgs.findutils
                darwinPkgs.e2fsprogs
                darwinPkgs.openssh
                darwinPkgs.nix
              ]
              ++ extraInputs;
              text = ''
                export KERNEL=${darwinPkgs.lib.escapeShellArg "${guest.config.system.build.kernel}/${guest.config.system.boot.loader.kernelFile}"}
                export INITRD=${darwinPkgs.lib.escapeShellArg "${guest.config.system.build.netbootRamdisk}/initrd"}
                export TOPLEVEL=${darwinPkgs.lib.escapeShellArg "${guest.config.system.build.toplevel}"}
                ${extraExports}
                exec bash ${script} "$@"
              '';
            };
          spike-a = mkSpike {
            name = "spike-a";
            guest = guestA;
            script = ./nix/spike-a.sh;
          };
          spike-b = mkSpike {
            name = "spike-b";
            guest = guestB;
            script = ./nix/spike-b.sh;
            extraExports = ''
              export HELLO=${darwinPkgs.lib.escapeShellArg "${hello}"}
              export SPIKE_B_KEY_SRC=${darwinPkgs.lib.escapeShellArg "${./nix/spike-b/id_ed25519}"}
            '';
          };
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
