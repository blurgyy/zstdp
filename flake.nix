{
  description = "Zstd proxy";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, ... }: flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system: let
    pkgs = import nixpkgs {
      inherit system;
      config = {
        # allowUnfree = true;
        # cudaSupport = true;
      };
      overlays = [
        self.overlays.default
      ];
    };
  in {
    packages = rec {
      inherit (pkgs) zstdp;
      default = zstdp;
    };
  }) // {
    overlays.default = final: prev: let
      version = "0.1.0";
    in {
      zstdp = final.callPackage ./. { inherit version; };
    };

    nixosModules.default = import ./nixos-module.nix;

    hydraJobs = self.packages;
  };
}
