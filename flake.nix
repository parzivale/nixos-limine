{
  description = "A limine bootloader installer for NixOS, in rust";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/8ce4ef6cb6f871616146b9fe26d2a5ae594e94fe";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-linux"
        "x86_64-linux"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      nixosModules.default = ./nix/module.nix;
      nixosModules.limine = self.nixosModules.default;

      overlays.default = final: _prev: {
        limine-install = final.callPackage ./nix/package.nix { };
      };

      packages = forAllSystems (pkgs: {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.limine-install;
        limine-install = pkgs.callPackage ./nix/package.nix { };
      });

      checks = forAllSystems (
        pkgs:
        nixpkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux (
          import ./tests {
            inherit pkgs;
            module = self.nixosModules.default;
          }
        )
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          name = "limine-install";

          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
          ];

          env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}
