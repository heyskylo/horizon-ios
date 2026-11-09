# The game workspace's sources: everything a `cargo build` at the root reads, without the folders it never
# does (the launcher is its own workspace), so editing those rebuilds nothing.
{ lib }:
let
  root = ../.;
  skipped = [
    ".github"
    ".githooks"
    ".claude"
    ".local"
    "benchmarks"
    "launcher"
    "nix"
    "target"
    "flake.nix"
    "flake.lock"
  ];
in
lib.cleanSourceWith {
  name = "horizon-source";
  src = lib.cleanSource root;
  filter =
    path: _type:
    let
      relative = lib.removePrefix (toString root + "/") (toString path);
    in
    !(lib.any (top: relative == top || lib.hasPrefix (top + "/") relative) skipped)
    # The pages of docs/ (the code reads only some of its data files).
    && !(lib.hasPrefix "docs/" relative && lib.hasSuffix ".md" relative);
}
