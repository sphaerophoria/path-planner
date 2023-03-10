with import (fetchTarball https://github.com/NixOS/nixpkgs/archive/81c6c120e6f8421783dba9334228591911bcc5b0.tar.gz) {};

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustup  rust-analyzer nodePackages_latest.vscode-langservers-extracted nodePackages_latest.eslint jsbeautifier nodejs wasm-pack binaryen linuxPackages_latest.perf
  ];
  LD_LIBRARY_PATH = with pkgs.xorg; "${libX11}/lib:${libXcursor}/lib:${libXxf86vm}/lib:${libXi}/lib:${libXrandr}/lib:${pkgs.libGL}/lib";
}

