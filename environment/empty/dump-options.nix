{ lib, options }:
let
  names = [
    "claude-code"
    "codex"
    "pi-coding-agent"
  ];
  flatten =
    prefix: set:
    lib.concatLists (
      lib.mapAttrsToList (
        name: val:
        let
          path = if prefix == "" then name else "${prefix}.${name}";
        in
        if name == "_module" then
          [ ]
        else if lib.isOption val then
          let
            ty = val.type.description or "json";
            tyL = lib.toLower ty;
            skipPkg = lib.hasInfix "package" tyL && !(lib.hasInfix "list" tyL);
          in
          if skipPkg || (val.internal or false) || (val.readOnly or false) then
            [ ]
          else
            let
              enc = builtins.tryEval (builtins.toJSON (val.default or null));
              default =
                if enc.success then builtins.fromJSON (builtins.unsafeDiscardStringContext enc.value) else null;
            in
            [
              {
                name = path;
                type = ty;
                inherit default;
                description = val.description or "";
                internal = false;
                readOnly = false;
              }
            ]
        else if builtins.isAttrs val then
          flatten path val
        else
          [ ]
      ) set
    );
  program = name: {
    inherit name;
    description = "home-manager programs.${name}";
    options = flatten "" (options.${name} or { });
  };
in
{
  programs = map program names;
}
