with import <nixpkgs> {};

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustup  rust-analyzer nodePackages_latest.vscode-langservers-extracted nodePackages_latest.eslint jsbeautifier
  ];
}

