# Spike A guest: Nix-built Linux that boots from kernel+initrd (netboot
# ramdisk holds the store). No virtiofs, no Host mounts. /workspace is a
# virtio-blk disk the Host creates empty and the guest formats.
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

  networking.hostName = "snowbox-spike-a";
  networking.useDHCP = false;
  networking.firewall.enable = false;
  networking.useNetworkd = false;

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
  ];
  boot.initrd.kernelModules = [
    "virtio_console"
    "virtio_blk"
  ];

  # First virtio-blk is /dev/vda. Host attaches a sparse raw image; we format
  # it on first boot so the Host does not need mkfs.ext4 on Darwin.
  fileSystems."/workspace" = {
    device = "/dev/vda";
    fsType = "ext4";
    autoFormat = true;
  };

  services.getty.autologinUser = "root";
  users.users.root.password = "";
  security.sudo.wheelNeedsPassword = false;

  environment.systemPackages = [
    pkgs.coreutils
    pkgs.util-linux
  ];

  # When the Host adds `spike.prove=1` to the cmdline, prove isolation and halt.
  systemd.services.spike-a-prove = {
    description = "Spike A isolation proof";
    wantedBy = [ "multi-user.target" ];
    after = [
      "local-fs.target"
      "workspace.mount"
    ];
    requires = [ "workspace.mount" ];
    serviceConfig = {
      Type = "oneshot";
      StandardOutput = "journal+console";
      StandardError = "journal+console";
    };
    script = ''
      set -eu
      if ! grep -q 'spike.prove=1' /proc/cmdline; then
        exit 0
      fi

      fail() {
        echo "SPIKE_A_FAIL $*"
        exit 1
      }

      echo "spike-a" > /workspace/marker || fail workspace_write
      test -f /workspace/marker || fail workspace_read

      src=$(${lib.getExe' pkgs.util-linux "findmnt"} -n -o SOURCE /workspace || true)
      fstype=$(${lib.getExe' pkgs.util-linux "findmnt"} -n -o FSTYPE /workspace || true)
      echo "SPIKE_A_WORKSPACE source=$src fstype=$fstype"
      echo "$fstype" | grep -qx ext4 || fail workspace_not_ext4
      echo "$src" | grep -q '/dev/' || fail workspace_not_block

      for p in /Users /Users/hcbt /home/hcbt; do
        if [ -e "$p" ]; then
          fail "host_path_visible:$p"
        fi
      done

      echo SPIKE_A_PASS
      ${lib.getExe' pkgs.systemd "systemctl"} poweroff --force --force
    '';
  };
}
