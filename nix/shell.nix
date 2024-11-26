{ packages, mkShell, nixfmt-rfc-style, rustup, }:

mkShell {
  inputsFrom = builtins.attrValues packages;
  packages = [ nixfmt-rfc-style rustup ];
}
