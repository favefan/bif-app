fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new()
                .commands(&["get_desktop_settings", "apply_desktop_settings"]),
        ),
    )
    .expect("build bif-app desktop permissions")
}
