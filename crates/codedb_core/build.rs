use std::env;
use std::fs;
use std::path::PathBuf;

use toml::Value as TomlValue;

fn main() {
    let workspace_root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir")).join("../..");
    let lock_path = workspace_root.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock_path.display());

    let lock = fs::read_to_string(&lock_path)
        .unwrap_or_else(|error| panic!("read workspace lockfile {}: {error}", lock_path.display()));
    let protocol_version = locked_nu_plugin_protocol_version(&lock)
        .unwrap_or_else(|error| panic!("derive Nushell plugin protocol version: {error}"));

    println!("cargo:rustc-env=CODEDB_NU_PLUGIN_PROTOCOL_VERSION={protocol_version}");
    println!(
        "cargo:rustc-env=CODEDB_NU_PLUGIN_PROTOCOL_NOTE=package targets nu-plugin/nu-protocol handshake {protocol_version}"
    );
}

fn locked_nu_plugin_protocol_version(lock: &str) -> Result<String, String> {
    let lock: TomlValue = toml::from_str(lock).map_err(|error| error.to_string())?;
    let packages = lock
        .get("package")
        .and_then(TomlValue::as_array)
        .ok_or_else(|| "Cargo.lock has no package entries".to_string())?;

    let plugin = unique_package(packages, "nu-plugin")?;
    let plugin_version = package_version(plugin, "nu-plugin")?;
    let protocol = unique_package(packages, "nu-plugin-protocol")?;
    let protocol_version = package_version(protocol, "nu-plugin-protocol")?;
    let nu_protocol = unique_package(packages, "nu-protocol")?;
    let nu_protocol_version = package_version(nu_protocol, "nu-protocol")?;

    let plugin_uses_handshake = plugin
        .get("dependencies")
        .and_then(TomlValue::as_array)
        .is_some_and(|dependencies| {
            dependencies.iter().any(|dependency| {
                dependency.as_str().is_some_and(|dependency| {
                    dependency == "nu-plugin-protocol"
                        || dependency.starts_with("nu-plugin-protocol ")
                })
            })
        });
    if !plugin_uses_handshake {
        return Err("locked nu-plugin does not depend on nu-plugin-protocol".to_string());
    }

    if plugin_version != protocol_version || nu_protocol_version != protocol_version {
        return Err(format!(
            "locked Nu dependency drift: nu-plugin={plugin_version}, \
             nu-plugin-protocol={protocol_version}, nu-protocol={nu_protocol_version}"
        ));
    }

    Ok(protocol_version.to_string())
}

fn unique_package<'a>(packages: &'a [TomlValue], name: &str) -> Result<&'a TomlValue, String> {
    let matches = packages
        .iter()
        .filter(|package| package.get("name").and_then(TomlValue::as_str) == Some(name))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "expected exactly one locked {name} package, found {}",
            matches.len()
        ));
    }
    Ok(matches[0])
}

fn package_version<'a>(package: &'a TomlValue, name: &str) -> Result<&'a str, String> {
    package
        .get("version")
        .and_then(TomlValue::as_str)
        .ok_or_else(|| format!("locked {name} package has no version"))
}
