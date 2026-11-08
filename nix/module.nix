{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.arcglyph;
in
{
  options.programs.arcglyph = {
    enable = lib.mkEnableOption "ArcGlyph mouse gestures";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "The ArcGlyph package to install.";
    };

    users = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "alice" ];
      description = ''
        Existing users to grant access to input devices and /dev/uinput.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    hardware.uinput.enable = true;

    users.users = lib.genAttrs cfg.users (_: {
      extraGroups = [
        "input"
        "uinput"
      ];
    });

    # Preserve login entries across system upgrades and garbage collection.
    systemd.tmpfiles.rules = [
      "L+ /usr/bin/arcglyph - - - - /run/current-system/sw/bin/arcglyph"
    ];
  };
}
