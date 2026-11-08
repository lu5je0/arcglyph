{
  description = "ArcGlyph mouse gestures for Wayland";

  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-26.05/nixexprs.tar.zst";

  outputs =
    { nixpkgs, ... }:
    {
      packages = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ] (
        system:
        let
          arcglyph = nixpkgs.legacyPackages.${system}.callPackage ./nix/package.nix { };
        in
        {
          inherit arcglyph;
          default = arcglyph;
        }
      );

      nixosModules.default = import ./nix/module.nix;
    };
}
