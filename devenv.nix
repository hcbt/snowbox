# Host-side shell. vfkit is the Virtualization.framework VMM (ADR 0010).
# Everyday utilities are pinned so `nix develop` / `devenv shell` do not fall
# through to Homebrew.
{ pkgs, ... }:
{
  packages = [
    pkgs.git
    pkgs.gh
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gnused
    pkgs.findutils
    pkgs.curl
    pkgs.vfkit
    pkgs.e2fsprogs
  ];

  git-hooks.package = pkgs.prek;
  git-hooks.hooks = {
    nixfmt-rfc-style.enable = true;
    nixfmt-rfc-style.package = pkgs.nixfmt;
    check-merge-conflicts.enable = true;
    check-yaml.enable = true;
    check-added-large-files.enable = true;
    end-of-file-fixer.enable = true;
    trim-trailing-whitespace.enable = true;
  };

  git-hooks.excludes = [
    "^LICENSE$"
    "\\.lock$"
  ];
}
