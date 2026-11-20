{
  description = "Render Markdown in the terminal as a browser shows them, styled like GitHub by default";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      system = "aarch64-darwin";
      pkgs = nixpkgs.legacyPackages.${system};
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages.${system} = {
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
            platforms = [ system ];
            mainProgram = "mdd";
          };
        };

        default = self.packages.${system}.mddog;
      };

      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ self.packages.${system}.mddog ];
        packages = with pkgs; [
          cargo
          clippy
          rustc
          rustfmt
        ];
      };
    };
}
