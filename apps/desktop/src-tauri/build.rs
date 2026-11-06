fn main() {
    // Only the app's one command is allowed from the window (D3 §3).
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["request"])),
    )
    .expect("tauri-build");
}
