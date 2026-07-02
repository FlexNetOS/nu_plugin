pub fn always_present() -> &'static str {
    "always"
}

#[cfg(feature = "extra")]
pub fn extra_feature() -> &'static str {
    "extra"
}

#[cfg(target_os = "linux")]
pub fn linux_only() -> &'static str {
    "linux"
}
