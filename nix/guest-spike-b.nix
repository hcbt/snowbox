# Spike B guest: same isolation as A (workspace on virtio-blk, no Host
# mounts) plus virtio-net + ssh so the Host Daemon can copy store paths
# in. Environment packages are NOT in this image; they arrive via nix copy.
{
  config,
  lib,
  pkgs,
  modulesPath,
  ...
}:
{
  imports = [
    "${modulesPath}/profiles/minimal.nix"
    "${modulesPath}/installer/netboot/netboot.nix"
  ];

  nixpkgs.hostPlatform = "aarch64-linux";
  system.stateVersion = lib.trivial.release;

  networking.hostName = "snowbox-spike-b";
  networking.firewall.enable = false;
  networking.useDHCP = false;
  networking.useNetworkd = true;
  systemd.network.wait-online.enable = false;
  systemd.network.networks."10-nat" = {
    matchConfig.Name = "en* eth*";
    networkConfig.DHCP = "yes";
  };

  documentation.enable = false;
  documentation.nixos.enable = false;
  documentation.man.enable = false;
  documentation.info.enable = false;
  documentation.doc.enable = false;

  boot.loader.grub.enable = false;
  boot.kernelParams = [
    "console=hvc0"
    "reboot=t"
    "panic=-1"
  ];
  boot.initrd.availableKernelModules = [
    "virtio_pci"
    "virtio_mmio"
    "virtio_blk"
    "virtio_console"
    "virtio_rng"
    "virtio_net"
  ];
  boot.initrd.kernelModules = [
    "virtio_console"
    "virtio_blk"
    "virtio_net"
  ];

  fileSystems."/workspace" = {
    device = "/dev/vda";
    fsType = "ext4";
    autoFormat = true;
  };

  services.getty.autologinUser = "root";
  users.users.root.password = "";
  security.sudo.wheelNeedsPassword = false;
  users.users.root.openssh.authorizedKeys.keys = [
    (builtins.readFile ./spike-b/id_ed25519.pub)
  ];

  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "prohibit-password";
      PasswordAuthentication = false;
    };
  };

  environment.systemPackages = [
    pkgs.coreutils
    pkgs.util-linux
    pkgs.iproute2
    pkgs.nix
  ];

  # Advertise DHCP address on the serial console so the Host does not have
  # to parse macOS dhcpd_leases.
  systemd.services.spike-b-ready = {
    description = "Spike B ready banner";
    wantedBy = [ "multi-user.target" ];
    after = [
      "local-fs.target"
      "workspace.mount"
      "network-online.target"
      "sshd.service"
    ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      StandardOutput = "journal+console";
      StandardError = "journal+console";
    };
    script = ''
      set -eu
      ip=""
      for _ in $(seq 1 40); do
        ip=$(${lib.getExe' pkgs.iproute2 "ip"} -4 -o addr show scope global \
          | ${lib.getExe' pkgs.gawk "awk"} '{print $4}' \
          | ${lib.getExe' pkgs.coreutils "cut"} -d/ -f1 \
          | ${lib.getExe' pkgs.coreutils "head"} -1 || true)
        if [ -n "$ip" ]; then
          break
        fi
        sleep 1
      done
      if [ -z "$ip" ]; then
        echo "SPIKE_B_FAIL no_ip"
        exit 1
      fi
      echo "SPIKE_B_IP=$ip"
      echo SPIKE_B_READY
    '';
  };
}
