{
  description = "EventModelRenderer development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            cargo-nextest
            git
            pre-commit
            nodejs_22
            glow
            just
            jq
            sqlx-cli
            postgresql
          ];

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

          shellHook = ''
            CARGO_MCP_VERSION="0.2.0"
            CARGO_AUDIT_VERSION="0.21.0"

            # Setup local cargo bin directory
            export CARGO_INSTALL_ROOT="$PWD/.cargo-bin"
            export PATH="$CARGO_INSTALL_ROOT/bin:$PATH"

            # Create directory if it doesn't exist
            mkdir -p "$CARGO_INSTALL_ROOT/bin"

            # Check cargo-mcp version
            if ! command -v cargo-mcp >/dev/null 2>&1 || [ "$(cargo-mcp --version 2>/dev/null | awk '{print $2}')" != "$CARGO_MCP_VERSION" ]; then
              echo "Installing cargo-mcp $CARGO_MCP_VERSION to $CARGO_INSTALL_ROOT..."
              cargo install cargo-mcp --version "$CARGO_MCP_VERSION" --root "$CARGO_INSTALL_ROOT"
            fi

            # Check cargo-audit version
            if ! command -v cargo-audit >/dev/null 2>&1 || [ "$(cargo-audit --version 2>/dev/null | awk '{print $2}')" != "$CARGO_AUDIT_VERSION" ]; then
              echo "Installing cargo-audit $CARGO_AUDIT_VERSION to $CARGO_INSTALL_ROOT..."
              cargo install cargo-audit --version "$CARGO_AUDIT_VERSION" --root "$CARGO_INSTALL_ROOT"
            fi

            # Use project-local advisory database
            alias cargo-audit='cargo audit --db "$PWD/.cargo-advisory-db"'
          '';
        };
      }
    );
}
