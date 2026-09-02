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
    starship.enable = true;
    starship.enableBashIntegration = true;
    starship.settings = {
      add_newline = false;
      format = "$username$hostname$directory$character";
      username = {
        show_always = true;
        format = "$user";
      };
      hostname = {
        ssh_only = false;
        format = "@$hostname";
      };
      directory = {
        format = ":$path";
        truncate_to_repo = false;
        truncation_length = 8;
      };
      character = {
        # Starship literal $ is \$; Nix [\$] drops the slash. $$ still fails to parse.
        # Style without a color so the Window xterm foreground follows Theme×Mode.
        success_symbol = "[\\$](bold)";
        error_symbol = "[\\$](bold)";
      };
    };
  }
  // lib.mapAttrs (_: apply) programs;
  home.packages = [ pkgs.devenv ];
}
