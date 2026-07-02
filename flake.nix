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
