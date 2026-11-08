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
    autostart = lib.mkEnableOption "starting ArcGlyph when the listed users log in to a Wayland session";

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

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        environment.systemPackages = [ cfg.package ];
        hardware.uinput.enable = true;

        users.users = lib.genAttrs cfg.users (_: {
          extraGroups = [
            "input"
            "uinput"
          ];
        });
      }
      (lib.mkIf cfg.autostart {
        assertions = [
          {
            assertion = cfg.users != [ ];
            message = "programs.arcglyph.autostart requires at least one user in programs.arcglyph.users.";
          }
        ];

        environment.etc."arcglyph/autostart-users".text = lib.concatLines cfg.users;

        systemd.user.services.arcglyph = {
          description = "ArcGlyph mouse gestures";
          wantedBy = [ "graphical-session.target" ];
          partOf = [ "graphical-session.target" ];
          after = [ "graphical-session.target" ];
          unitConfig = {
            # Let the desktop reach its target before launching the Wayland client.
            DefaultDependencies = false;
            ConditionEnvironment = "WAYLAND_DISPLAY";
            ConditionUser = map (user: "|${user}") cfg.users;
          };
          serviceConfig = {
            Type = "exec";
            ExecStart = "${lib.getExe' cfg.package "arcglyph"} --hidden";
          };
        };
      })
    ]
  );
}
