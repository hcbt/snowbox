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
    starship.enable = true;
    starship.enableBashIntegration = true;
    starship.settings = {
      add_newline = false;
      format = "$username$hostname$directory$character";
      username = {
        show_always = true;
        format = "[$user]($style)";
        style_user = "#ededef";
        style_root = "#ededef";
      };
      hostname = {
        ssh_only = false;
        format = "[@$hostname]($style)";
        style = "#ededef";
      };
      directory = {
        format = "[:$path]($style)";
        style = "#ededef";
        truncate_to_repo = false;
        truncation_length = 8;
      };
      character = {
        success_symbol = "[\$](#ededef)";
        error_symbol = "[\$](#ededef)";
      };
    };
  }
  // lib.mapAttrs (_: apply) programs;
  home.packages = [ pkgs.devenv ];
}
