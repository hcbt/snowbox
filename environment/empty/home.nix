{ pkgs, lib, ... }:
let
  raw = builtins.fromJSON (builtins.readFile ./config.json);
  programs = raw.programs or { };
  withPkgs =
    cfg:
    let
      names = cfg.extraPackages or [ ];
    in
    (builtins.removeAttrs cfg [ "extraPackages" ])
    // {
      extraPackages = map (n: pkgs.${n}) names;
    };
  apply = cfg: if cfg ? extraPackages then withPkgs cfg else cfg;
in
{
  home.username = "snow";
  home.homeDirectory = "/home/snow";
  home.stateVersion = "26.05";
  programs = {
    bash.enable = true;
    bash.initExtra = ''
      if [ -f "$HOME/.snowbox-env" ]; then
        . "$HOME/.snowbox-env"
      fi
    '';
  }
  // lib.mapAttrs (_: apply) programs;
  home.packages = [ pkgs.devenv ];
}
