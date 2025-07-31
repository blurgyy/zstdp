{
  description = "Zstd proxy";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    let
      deps = import ./deps;
    in
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            self.overlays.default
            self.overlays.deps
          ];
        };
      in
      {
        packages = (deps.packages pkgs) // {
          inherit (pkgs) zstdp;
          default = pkgs.zstdp;
        };
      }
    )
    // {
      overlays = {
        default =
          final: prev:
          let
            mtime = self.lastModifiedDate;
            date = "${builtins.substring 0 4 mtime}-${builtins.substring 4 2 mtime}-${builtins.substring 6 2 mtime}";
            rev = self.rev or (nixpkgs.lib.warn "Git changes are not committed" (self.dirtyRev or "dirty"));
            version = "${date}+${builtins.substring 0 8 rev}";
          in
          {
            zstdp = final.callPackage ./. { inherit version; };
          };
        deps = deps.overlay;
      };

      nixosModules.default = import ./nixos-module.nix;

      hydraJobs = self.packages;
    };
}
