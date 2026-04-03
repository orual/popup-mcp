{
  description = "popup-mcp: Native GUI popups via MCP";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      inherit (pkgs) lib;

      rustToolchain = pkgs.rust-bin.stable.latest.default;
      craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

      # Runtime libraries needed by egui/eframe (loaded via dlopen)
      runtimeLibs = with pkgs; [
        libxkbcommon
        libGL
        fontconfig
        wayland
        libx11
        libxcursor
        libxrandr
        libxi
      ];

      # Build-time native dependencies
      nativeDeps = with pkgs; [
        pkg-config
      ];

      # Build-time library dependencies
      buildDeps = with pkgs; [
        openssl
      ] ++ runtimeLibs;

      # Include .pest files alongside standard cargo sources
      pestFilter = path: _type: builtins.match ".*\\.pest$" path != null;
      src = lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          (pestFilter path type) || (craneLib.filterCargoSources path type);
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
        nativeBuildInputs = nativeDeps;
        buildInputs = buildDeps;

        # Ensure the linker can find libraries during build
        LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      popup = craneLib.buildPackage (commonArgs
        // {
          inherit cargoArtifacts;
          # Wrap the binary so it can find runtime libraries
          nativeBuildInputs = nativeDeps ++ [pkgs.makeWrapper];
          postInstall = ''
            wrapProgram $out/bin/popup \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
          '';
        });
    in {
      checks = {
        inherit popup;

        popup-clippy = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

        popup-fmt = craneLib.cargoFmt {inherit src;};
      };

      packages = {
        default = popup;
        inherit popup;
      };

      apps.default = flake-utils.lib.mkApp {drv = popup;};

      devShells.default = craneLib.devShell {
        checks = self.checks.${system};
        packages = with pkgs; [
          cargo-watch
        ];
        LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
      };
    });
}
