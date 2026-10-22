{
  description = "PlanTool development environment";

  inputs = {
    nixpkgs.url = "https://codeload.github.com/NixOS/nixpkgs/tar.gz/ac62194c3917d5f474c1a844b6fd6da2db95077d";
    rust-overlay = {
      url = "https://codeload.github.com/oxalica/rust-overlay/tar.gz/83dd732f2428827f35d997deae108a2ced5125f4";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      shellFor = system:
        let pkgs = pkgsFor system;
        in pkgs.mkShell {
          packages = with pkgs; [
            (rust-bin.stable.latest.default.override {
              extensions = [ "rust-src" "rustfmt" "clippy" ];
            })
            nodejs_22
            python312
            uv
            sqlite
            gh
            cargo-nextest
          ];
        };
    in {
      devShells = forAllSystems (system: { default = shellFor system; });
      # Build the shell's dependencies as well as evaluating its definition.
      checks = forAllSystems (system: { dev-shell = shellFor system; });
    };
}
