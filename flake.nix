{
  description = "Edgehog device runtime";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        packages = self.packages.${system};
        inherit (pkgs) lib callPackage;
      in
      {
        packages = {
          default = callPackage ./nix/package.nix { };
          openwrt-aarch64-musl = pkgs.pkgsCross.aarch64-multiplatform-musl.callPackage ./nix/package.nix {
            fullStatic = true;
            buildNoDefaultFeatures = true;
            buildFeatures = [
              "forwarder"
              "containers"
            ];
          };
        };
        apps.default = {
          type = "app";
          program = lib.getExe packages.default;
        };
        devShells.default = callPackage ./nix/shell.nix { inherit packages; };
      }
    );
}
