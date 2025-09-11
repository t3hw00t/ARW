{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
            jq
            just
            cargo-watch
            rust-bin.stable.latest.default
            rust-bin.stable.latest.rustfmt
            rust-bin.stable.latest.clippy
            cargo-nextest
            cargo-audit
            cargo-deny
            python3Packages.mkdocs
            python3Packages.mkdocs-material
            python3Packages.mkdocs-git-revision-date-localized-plugin
          ];
          buildInputs = with pkgs; [
            gtk3
            xdotool
          ];
        };
      }
    );
}
