let
  pkgs = import <nixpkgs> {};
in with pkgs; stdenv.mkDerivation rec {
  name = "rust-gpu-lodestar";

  # Workaround for https://github.com/NixOS/nixpkgs/issues/60919.
  # NOTE(eddyb) needed only in debug mode (warnings about needing optimizations
  # turn into errors due to `-Werror`, for at least `spirv-tools-sys`).
  hardeningDisable = [ "fortify" ];

  # Allow cargo to download crates (even inside `nix-shell --pure`).
  SSL_CERT_FILE = "${cacert}/etc/ssl/certs/ca-bundle.crt";

  nativeBuildInputs = [ rustup ];

  # Runtime dependencies (for e.g. `wgpu`).
  LD_LIBRARY_PATH = lib.makeLibraryPath [ vulkan-loader ];
}
