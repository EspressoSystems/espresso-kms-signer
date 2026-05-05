{
  description = "Espresso KMS Signing Sidecar";

  nixConfig = {
    extra-substituters = [
      "https://espresso-systems-private.cachix.org"
    ];
    extra-trusted-public-keys = [
      "espresso-systems-private.cachix.org-1:LHYk03zKQCeZ4dvg3NctyCq88e44oBZVug5LpYKjPRI="
    ];
  };

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs =
    { self
    , nixpkgs
    , rust-overlay
    , flake-utils
    , ...
    }:
    flake-utils.lib.eachDefaultSystem (system:
    let
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs { inherit system overlays; };

      rustToolchain = pkgs.rust-bin.stable.latest.default;

      buildInputs = with pkgs; [
        openssl
        pkg-config
      ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
        pkgs.apple-sdk_15
      ];
    in
    {
      devShells.default = pkgs.mkShell {
        buildInputs = [ rustToolchain ] ++ buildInputs;
        shellHook = ''
          echo "espresso-kms-signer dev shell"
          echo "Run 'cargo build' to compile or 'cargo test' to run tests."
        '';
      };
    });
}
