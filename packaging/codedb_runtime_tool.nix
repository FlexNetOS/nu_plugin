{
  lib,
  rustPlatform,
}:

let
  packageVersion = "0.1.0";
  cargoPackageFlags = [
    "-p"
    "codedb"
    "-p"
    "nu_plugin_codedb"
  ];
in
rustPlatform.buildRustPackage {
  pname = "codedb-runtime-tools";
  version = packageVersion;

  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      let
        rel = lib.removePrefix ((toString ../.) + "/") (toString path);
      in
      !(
        type == "directory"
        && builtins.elem rel [
          "target"
          ".git"
        ]
      );
  };

  cargoLock.lockFile = ../Cargo.lock;

  cargoBuildFlags = cargoPackageFlags;

  cargoTestFlags = cargoPackageFlags;

  installPhase = ''
    runHook preInstall

    codedb_bin="$(find target -path '*/release/codedb' -type f -perm -0100 | head -n 1)"
    plugin_bin="$(find target -path '*/release/nu_plugin_codedb' -type f -perm -0100 | head -n 1)"
    if [ -z "$codedb_bin" ] || [ -z "$plugin_bin" ]; then
      echo "error: expected codedb and nu_plugin_codedb release binaries under target/" >&2
      find target -maxdepth 4 -type f | sort >&2
      exit 1
    fi

    install -Dm755 "$codedb_bin" "$out/bin/codedb"
    install -Dm755 "$plugin_bin" "$out/bin/nu_plugin_codedb"

    mkdir -p "$out/share/codedb"
    cat > "$out/share/codedb/runtime-tool-metadata.json" <<JSON
    {
      "schema_version": 1,
      "package_name": "codedb-runtime-tools",
      "version": "${packageVersion}",
      "commands": ["codedb", "nu_plugin_codedb"],
      "runtime_tool_source": "bundled",
      "codedb_bin": "$out/bin/codedb",
      "codedb_nu_plugin_bin": "$out/bin/nu_plugin_codedb"
    }
    JSON

    runHook postInstall
  '';

  doCheck = true;

  passthru.runtimeToolMetadata = {
    schema_version = 1;
    package_name = "codedb-runtime-tools";
    commands = [
      "codedb"
      "nu_plugin_codedb"
    ];
    runtime_tool_source = "bundled";
    env = {
      YAZELIX_CODEDB_BIN = "bin/codedb";
      YAZELIX_CODEDB_PLUGIN_BIN = "bin/nu_plugin_codedb";
    };
  };

  meta = {
    description = "CodeDB CLI and Nushell plugin runtime tool package";
    license = lib.licenses.mit;
    mainProgram = "codedb";
  };
}
