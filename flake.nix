{
  description = "CodeDB Nushell plugin and CLI runtime package";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          codedbRuntimeTools = pkgs.callPackage ./packaging/codedb_runtime_tool.nix { };
        in
        {
          default = codedbRuntimeTools;
          codedb_runtime_tools = codedbRuntimeTools;
          codedb = codedbRuntimeTools;
          nu_plugin_codedb = codedbRuntimeTools;
        }
      );

      checks = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          runtimeTools = self.packages.${system}.codedb_runtime_tools;
          source = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              let
                rel = pkgs.lib.removePrefix ((toString ./.) + "/") (toString path);
              in
              !(
                type == "directory"
                && builtins.elem rel [
                  "target"
                  ".git"
                  ".kb/.cache"
                  ".kb/store"
                  ".kb/workspaces"
                ]
              );
          };
        in
        {
          codedb_runtime_tool_smoke = pkgs.runCommand "codedb-runtime-tool-smoke" { } ''
            set -eu
            ${runtimeTools}/bin/codedb --version > codedb-version.txt
            test -x ${runtimeTools}/bin/nu_plugin_codedb
            printf '%s\n' "${runtimeTools}/bin/nu_plugin_codedb" > plugin-path.txt
            grep -F "nu_plugin_codedb" ${runtimeTools}/share/codedb/runtime-tool-metadata.json
            grep -F "${runtimeTools.version}" codedb-version.txt
            mkdir -p "$out"
            cp codedb-version.txt plugin-path.txt "$out"/
          '';

          repo_truth_surface =
            pkgs.runCommand "codedb-repo-truth-surface"
              {
                nativeBuildInputs = [ pkgs.git ];
              }
              ''
                set -eu
                cp -R ${source} source
                chmod -R u+w source
                cd source
                git init --quiet
                git add --all
                ${pkgs.python3}/bin/python3 scripts/truth_surface.py --check-source
                mkdir -p "$out"
                printf '%s\n' "repo truth surface ok" > "$out/result.txt"
              '';

          import_rows_smoke = pkgs.rustPlatform.buildRustPackage {
            pname = "codedb-import-rows-smoke";
            version = "0.1.0";
            src = source;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [
              "-p"
              "nu_plugin_codedb"
            ];
            cargoTestFlags = [
              "-p"
              "nu_plugin_codedb"
              "import_rows"
            ];
            installPhase = ''
              mkdir -p "$out"
              printf '%s\n' "import rows smoke ok" > "$out/result.txt"
            '';
          };

          nushell_syntax_smoke = pkgs.runCommand "codedb-nushell-syntax-smoke" { } ''
            set -eu
            cp -R ${source} source
            chmod -R u+w source
            cd source
            export PATH="${pkgs.nushell}/bin:$PATH"
            ${pkgs.nushell}/bin/nu tests/test_nushell_syntax_gate.nu
            mkdir -p "$out"
            printf '%s\n' "nushell syntax smoke ok" > "$out/result.txt"
          '';
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = self.devShells.${system}.ci;
          ci = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.clippy
              pkgs.rustc
              pkgs.rustfmt
              pkgs.nushell
              pkgs.python3
              pkgs.nixfmt
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              pkgs.bubblewrap
            ];

            CODEDB_CI_SHELL = "1";
          };
        }
      );

      formatter = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        pkgs.writeShellApplication {
          name = "codedb-nixfmt";
          runtimeInputs = [ pkgs.nixfmt ];
          text = ''
            if [ "$#" -eq 0 ]; then
              set -- flake.nix packaging/*.nix
            fi
            exec nixfmt "$@"
          '';
        }
      );
    };
}
