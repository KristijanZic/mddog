{
  description = "Render Markdown in the terminal as a browser shows them, styled like GitHub by default";

  inputs = {
    # 26.05 is the latest branch supporting both aarch64-darwin and x86_64-darwin
    # (nixpkgs-unstable / 26.11 dropped x86_64-darwin).
    # On aarch64-darwin, consumers can set `inputs.nixpkgs.follows = "nixpkgs"` to use latest nixpkgs.
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      cargoToml = fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          mddog = pkgs.rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;

            src = pkgs.lib.cleanSource ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            checkFlags = [
              # Requires macOS /usr/bin/defaults (unavailable in the Nix sandbox).
              "--skip=help_follows_an_appearance_change"
              # Invokes macOS BSD `base64 -D`, whereas Nix's stdenv provides GNU `base64`.
              "--skip=cmd_c_copies_the_selection"
              "--skip=copy_on_select_copies_a_double_clicked_word"
              "--skip=copy_on_select_copies_a_drag"
            ];

            meta = {
              description = cargoToml.package.description;
              homepage = "https://github.com/qu8n/mddog";
              license = pkgs.lib.licenses.mit;
              platforms = systems;
              mainProgram = "mdd";
            };
          };

          default = self.packages.${system}.mddog;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.mddog ];
            packages = with pkgs; [
              cargo
              clippy
              rustc
              rustfmt
            ];
          };
        }
      );
    };
}
