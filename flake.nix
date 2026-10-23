{
  description = "PlanTool development environment";

  inputs = {
    nixpkgs.url = "https://codeload.github.com/NixOS/nixpkgs/tar.gz/ac62194c3917d5f474c1a844b6fd6da2db95077d";
    rust-overlay = {
      url = "https://codeload.github.com/oxalica/rust-overlay/tar.gz/83dd732f2428827f35d997deae108a2ced5125f4";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "https://codeload.github.com/ipetkov/crane/tar.gz/2b3a5a88d852575758e1eb6ac9ee677fcd633fc1";
    pyproject-nix = {
      url = "https://codeload.github.com/nix-community/pyproject.nix/tar.gz/ddc400a6883b042646d0940e65971bb7f7e96107";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    uv2nix = {
      url = "https://codeload.github.com/pyproject-nix/uv2nix/tar.gz/0fdaa03ea3c3c16dbb1fe4d69108f07227a4b42d";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pyproject-build-systems = {
      url = "https://codeload.github.com/pyproject-nix/build-system-pkgs/tar.gz/a17819c85163988e063806ffc4f6d2716bc8ead8";
      inputs.uv2nix.follows = "uv2nix";
      inputs.pyproject-nix.follows = "pyproject-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, crane, uv2nix, pyproject-nix, pyproject-build-systems, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      uvFor = system:
        let
          pkgs = pkgsFor system;
          manifest = builtins.fromJSON (builtins.readFile ./crates/daemon/src/worker/uv-checksums.json);
          target = {
            x86_64-linux = "x86_64-unknown-linux-musl";
            aarch64-linux = "aarch64-unknown-linux-musl";
            x86_64-darwin = "x86_64-apple-darwin";
            aarch64-darwin = "aarch64-apple-darwin";
          }.${system};
        in pkgs.stdenvNoCC.mkDerivation {
          pname = "uv";
          version = manifest.version;
          src = pkgs.fetchurl {
            url = "https://github.com/astral-sh/uv/releases/download/${manifest.version}/uv-${target}.tar.gz";
            sha256 = manifest.targets.${target};
          };
          dontConfigure = true;
          dontBuild = true;
          installPhase = "mkdir -p $out/bin; cp uv uvx $out/bin";
        };
      packageFor = system:
        let
          pkgs = pkgsFor system;
          lib = pkgs.lib;
          toolchain = pkgs.rust-bin.stable.latest.default;
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
          workspace = uv2nix.lib.workspace.loadWorkspace { workspaceRoot = ./worker; };
          pythonSet = (pkgs.callPackage pyproject-nix.build.packages { python = pkgs.python312; }).overrideScope
            (lib.composeManyExtensions [
              pyproject-build-systems.overlays.default
              (workspace.mkPyprojectOverlay { sourcePreference = "wheel"; })
            ]);
          workerEnv = pythonSet.mkVirtualEnv "plantool-worker" workspace.deps.default;
          web = pkgs.buildNpmPackage {
            pname = "plantool-web";
            version = "0.1.0-alpha.1";
            src = ./web;
            nodejs = pkgs.nodejs_22;
            npmDeps = pkgs.importNpmLock { npmRoot = ./web; };
            npmConfigHook = pkgs.importNpmLock.npmConfigHook;
            installPhase = "cp -r dist $out";
          };
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml ./Cargo.lock ./crates ./agents ./skills ./contract ./tests
              ./worker/pyproject.toml ./worker/uv.lock
              (lib.fileset.fileFilter (file: file.hasExt "py" || file.name == "versions.json") ./worker/plantool_worker)
            ];
          };
          common = {
            inherit src;
            pname = "plantool";
            version = "0.1.0-alpha.1";
            strictDeps = true;
            postUnpack = "mkdir -p $sourceRoot/web; cp -r ${web} $sourceRoot/web/dist";
            nativeBuildInputs = [ pkgs.pkg-config pkgs.perl ];
            buildInputs = lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
            PLANTOOL_PACKAGED_WORKER_PYTHON = "${workerEnv}/bin/python";
          };
          cargoArtifacts = craneLib.buildDepsOnly common;
        in craneLib.buildPackage (common // {
          inherit cargoArtifacts;
          pname = "plantool";
          version = "0.1.0-alpha.1";
          cargoExtraArgs = "-p plantool";
          doCheck = false; # The test jobs need sockets; CI runs them separately.
          meta.mainProgram = "plantool";
        });
      shellFor = system:
        let pkgs = pkgsFor system;
        in pkgs.mkShell {
          packages = with pkgs; [
            (rust-bin.stable.latest.default.override {
              extensions = [ "rust-src" "rustfmt" "clippy" ];
            })
            nodejs_22
            python312
            (uvFor system)
            sqlite
            gh
            perl
            cargo-nextest
          ];
        };
    in {
      packages = forAllSystems (system: { default = packageFor system; });
      apps = forAllSystems (system: { default = { type = "app"; program = "${self.packages.${system}.default}/bin/plantool"; }; });
      devShells = forAllSystems (system: { default = shellFor system; });
      # Build the shell's dependencies as well as evaluating its definition.
      checks = forAllSystems (system: { dev-shell = shellFor system; package = self.packages.${system}.default; });
    };
}
