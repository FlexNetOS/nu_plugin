{
  bubblewrap,
  lib,
  makeWrapper,
  rustPlatform,
}:

let
  packageVersion = "0.1.0";
  cargoPackageFlags = [
    "-p"
    "codedb"
    "-p"
    "nu_plugin_codedb"
    "-p"
    "flexnetos-redb-owner"
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

  # Kernel-enforced compiler/build integration runs in the host/CI test lane.
  # A Nix build sandbox cannot create the nested user namespace bubblewrap
  # requires, so this package derivation runs the selected binaries' unit
  # targets and leaves the non-skipping integration suite to `cargo test
  # --workspace --all-features` outside the nested sandbox.
  cargoTestFlags = cargoPackageFlags ++ [ "--bins" ];

  nativeBuildInputs = [ makeWrapper ];

  installPhase = ''
    runHook preInstall

    codedb_bin="$(find target -path '*/release/codedb' -type f -perm -0100 | head -n 1)"
    plugin_bin="$(find target -path '*/release/nu_plugin_codedb' -type f -perm -0100 | head -n 1)"
    owner_bin="$(find target -path '*/release/flexnetos-redb-owner' -type f -perm -0100 | head -n 1)"
    if [ -z "$codedb_bin" ] || [ -z "$plugin_bin" ] || [ -z "$owner_bin" ]; then
      echo "error: expected codedb, nu_plugin_codedb, and flexnetos-redb-owner release binaries under target/" >&2
      find target -maxdepth 4 -type f | sort >&2
      exit 1
    fi

    install -Dm755 "$codedb_bin" "$out/bin/codedb"
    install -Dm755 "$plugin_bin" "$out/bin/nu_plugin_codedb"
    install -Dm755 "$owner_bin" "$out/bin/flexnetos-redb-owner"
    wrapProgram "$out/bin/codedb" \
      --prefix PATH : ${lib.makeBinPath [ bubblewrap ]}

    mkdir -p "$out/share/systemd/user"
    cat > "$out/share/systemd/user/flexnetos-redb-owner.service" <<UNIT
    [Unit]
    Description=FlexNetOS single-owner redb state service

    [Service]
    Type=simple
    ExecStart=$out/bin/flexnetos-redb-owner serve %h/meta/var/lib/redb
    Restart=on-failure
    RestartSec=1s
    UMask=0077
    NoNewPrivileges=yes

    [Install]
    WantedBy=default.target
    UNIT

    mkdir -p "$out/share/codedb"
    cat > "$out/share/codedb/runtime-tool-metadata.json" <<JSON
    {
      "schema_version": 1,
      "package_name": "codedb-runtime-tools",
      "version": "${packageVersion}",
      "commands": ["codedb", "nu_plugin_codedb", "flexnetos-redb-owner"],
      "runtime_tool_source": "bundled",
      "codedb_bin": "$out/bin/codedb",
      "codedb_nu_plugin_bin": "$out/bin/nu_plugin_codedb",
      "flexnetos_redb_owner_bin": "$out/bin/flexnetos-redb-owner",
      "flexnetos_redb_owner_unit": "$out/share/systemd/user/flexnetos-redb-owner.service"
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
      "flexnetos-redb-owner"
    ];
    runtime_tool_source = "bundled";
    env = {
      YAZELIX_CODEDB_BIN = "bin/codedb";
      YAZELIX_CODEDB_PLUGIN_BIN = "bin/nu_plugin_codedb";
      FLEXNETOS_REDB_OWNER_BIN = "bin/flexnetos-redb-owner";
    };
  };

  meta = {
    description = "CodeDB CLI, Nushell plugin, and single-owner redb runtime package";
    license = lib.licenses.mit;
    mainProgram = "codedb";
  };
}
