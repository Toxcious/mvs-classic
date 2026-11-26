//! Links DXVK-native's d3d11 and dxgi libraries on Linux, found through pkg-config (`dxvk-d3d11.pc`,
//! `dxvk-dxgi.pc`). Windows links the system's through the windows crate and needs nothing here.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        return;
    }
    for lib in ["dxvk-d3d11", "dxvk-dxgi"] {
        if let Err(e) = pkg_config::Config::new().probe(lib) {
            panic!(
                "DXVK-native's `{lib}` was not found through pkg-config ({e}). Build it with \
                 `tools/linux-build.sh deps` and build through `tools/linux-build.sh cargo`, which points \
                 PKG_CONFIG_PATH at it."
            );
        }
    }
}
