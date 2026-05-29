{
  description = "nsa — Discord bot: logs messages to Postgres, rewrites embed-unfriendly URLs";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    scaffold.url = "github:KNOXDEV/scaffold.nix";
    scaffold.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs:
    inputs.scaffold.lib.mkFlake {
      inherit inputs;
      src = ./nix;
      systems = ["x86_64-linux" "aarch64-linux"];

      exports = {
        packages = {pkgs, ...}: {
          default = pkgs.internal.nsa;
          inherit (pkgs.internal) nsa;
        };

        nixosModules = {modules, ...}: {
          default = modules.nixos.nsa;
          inherit (modules.nixos) nsa;
        };

        shells = {shells, ...}: {
          inherit (shells) default;
        };
      };
    };
}
