fn main() {
    // Windows resources and the default window icon must be rebuilt when artwork changes.
    // tauri-build tracks configuration, but does not track these icon inputs itself.
    for path in [
        "icon.svg",
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
        "icons/icon.ico",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    tauri_build::build()
}
