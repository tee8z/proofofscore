{
  description = "Astroid Comp - Competitive game server with Nostr authentication";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Use latest stable Rust with WASM target
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          targets = [ "wasm32-unknown-unknown" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Common build dependencies
        commonNativeBuildInputs = with pkgs; [
          pkg-config
        ];

        commonBuildInputs = with pkgs; [
          openssl
          sqlite
        ];

        # Source filtering
        src = pkgs.lib.cleanSourceWith {
          src = craneLib.path ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (builtins.match ".*\\.sql$" path != null)
            || (builtins.match ".*migrations.*" path != null)
            || (builtins.match ".*\\.html$" path != null)
            || (builtins.match ".*\\.js$" path != null)
            || (builtins.match ".*\\.css$" path != null)
            || (builtins.match ".*\\.svg$" path != null);
        };

        # Common environment variables
        commonEnvs = {
          SQLX_OFFLINE = "true";
          OPENSSL_NO_VENDOR = "1";
          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
          OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        };

        # Build workspace dependencies once (for caching)
        workspaceDeps = craneLib.buildDepsOnly ({
          pname = "astroid-comp-workspace-deps";
          version = "0.1.0";
          inherit src;
          buildInputs = commonBuildInputs;
          nativeBuildInputs = commonNativeBuildInputs;
        } // commonEnvs);

        # Server binary
        server = craneLib.buildPackage ({
          pname = "server";
          version = "0.1.0";
          inherit src;
          cargoArtifacts = workspaceDeps;
          buildInputs = commonBuildInputs;
          nativeBuildInputs = commonNativeBuildInputs;
          cargoExtraArgs = "--bin server";
        } // commonEnvs);

        # Development shell
        devShell = pkgs.mkShell {
          buildInputs = commonBuildInputs ++ [
            rustToolchain
            pkgs.just
            pkgs.sqlx-cli
            pkgs.wasm-pack
            pkgs.pkg-config
            pkgs.sqlite
            pkgs.openssl
          ];

          nativeBuildInputs = commonNativeBuildInputs;

          shellHook = ''
            export DATABASE_URL="sqlite:data/astroid.db"
            mkdir -p data

            echo ""
            echo "Astroid Comp Development Environment"
            echo "====================================="
            echo ""
            echo "Commands:"
            echo "  just build       - Build all crates"
            echo "  just build-wasm  - Build WASM module"
            echo "  just run         - Run the server"
            echo "  just check       - Run fmt, clippy, tests"
            echo ""
          '';

          SQLX_OFFLINE = "true";
        };

      in {
        packages = {
          default = server;
          inherit server;
        };

        apps = {
          server = flake-utils.lib.mkApp {
            drv = server;
            name = "server";
          };
          default = flake-utils.lib.mkApp {
            drv = server;
            name = "server";
          };
        };

        devShells.default = devShell;

        checks = {
          inherit server;

          clippy = craneLib.cargoClippy ({
            inherit src;
            cargoArtifacts = workspaceDeps;
            buildInputs = commonBuildInputs;
            nativeBuildInputs = commonNativeBuildInputs;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          } // commonEnvs);

          fmt = craneLib.cargoFmt { inherit src; };
        };
      }
    );
}
