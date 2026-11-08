//! Embeds the Windows manifest: release builds require administrator rights (ADR-0003);
//! debug builds run as the invoker, so the app starts from a normal development shell.

fn main() {
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    let manifest = if release {
        include_str!("windows/release.manifest")
    } else {
        include_str!("windows/debug.manifest")
    };
    println!("cargo:rerun-if-changed=windows/release.manifest");
    println!("cargo:rerun-if-changed=windows/debug.manifest");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .unwrap_or_else(|e| panic!("tauri build script failed: {e}"));
}
