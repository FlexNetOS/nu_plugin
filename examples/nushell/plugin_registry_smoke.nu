# Run from the CodeDB package root after building `nu_plugin_codedb`.
let temp_home = (mktemp -d)
let plugin = ((pwd) | path join target debug nu_plugin_codedb)
let plugin_config = ($temp_home | path join plugins.msgpackz)

with-env {
    HOME: $temp_home,
    XDG_CONFIG_HOME: ($temp_home | path join ".config"),
    XDG_DATA_HOME: ($temp_home | path join ".local/share"),
    XDG_CACHE_HOME: ($temp_home | path join ".cache"),
} {
    nu --no-config-file -c $"plugin add --plugin-config '($plugin_config)' '($plugin)'"
    nu --no-config-file --plugin-config $plugin_config -c $"plugin use --plugin-config '($plugin_config)' codedb; codedb tables"
}
