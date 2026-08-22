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

      packages.${darwin} =
        let
          guest = self.nixosConfigurations.spike-a;
          kernelFile = guest.config.system.boot.loader.kernelFile;
          kernel = "${guest.config.system.build.kernel}/${kernelFile}";
          initrd = "${guest.config.system.build.netbootRamdisk}/initrd";
          toplevel = "${guest.config.system.build.toplevel}";
          spike-a = darwinPkgs.writeShellApplication {
            name = "spike-a";
            runtimeInputs = [
              darwinPkgs.vfkit
              darwinPkgs.coreutils
              darwinPkgs.curl
              darwinPkgs.gnugrep
              darwinPkgs.e2fsprogs
            ];
            text = ''
              export KERNEL=${darwinPkgs.lib.escapeShellArg kernel}
              export INITRD=${darwinPkgs.lib.escapeShellArg initrd}
              export TOPLEVEL=${darwinPkgs.lib.escapeShellArg toplevel}
              exec bash ${./nix/spike-a.sh} "$@"
            '';
          };
        in
        {
          inherit spike-a;
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
      };
    };
}
