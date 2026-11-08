{
  description = "minesweepe-rs dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    ...
  }: let
    systems = ["x86_64-linux"];
    forAllSystems = f:
      nixpkgs.lib.genAttrs systems (
        system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [rust-overlay.overlays.default];
            }
          )
      );
  in {
    packages = forAllSystems (
      pkgs: let
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        customRustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
      in {
        default = customRustPlatform.buildRustPackage {
          pname = "minesweepe-rs";
          version = "0.4.4";

          src = pkgs.lib.cleanSource ./.;

          cargoLock.lockFile = ./Cargo.lock;
        };
      }
    );
    devShells = forAllSystems (pkgs: {
      default = pkgs.mkShell {
        packages = [
          # toolchain + components come from rust-toolchain.toml
          (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
        ];
      };
    });
  };
}
