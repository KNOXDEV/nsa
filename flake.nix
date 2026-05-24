{
  description = "nsa — Discord bot: logs messages to Postgres, rewrites embed-unfriendly URLs";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";

  outputs = {
    self,
    nixpkgs,
  }: let
    systems = ["x86_64-linux" "aarch64-linux"];
    forSystems = nixpkgs.lib.genAttrs systems;
    pkgsFor = system: nixpkgs.legacyPackages.${system};
  in {
    packages = forSystems (system: {
      default = (pkgsFor system).callPackage ./package.nix {};
      nsa = (pkgsFor system).callPackage ./package.nix {};
    });

    overlays.default = final: _prev: {
      nsa = final.callPackage ./package.nix {};
    };

    nixosModules.default = ./module.nix;
  };
}
