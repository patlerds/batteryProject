fn main() {
    // Register custom commands with Tauri's ACL instead of leaving them outside
    // the permission system. Only the local main window gets the allowlist.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "get_status",
        "save_config",
        "set_simulation",
        "set_simulated_battery",
        "restore_defaults",
        "export_diagnostics",
        "set_launch_at_login",
        "get_launch_at_login",
        "open_dell_support",
    ]);
    let windows =
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("windows.manifest"));
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(manifest)
            .windows_attributes(windows),
    )
    .expect("Could not build the app's restricted command manifest");
}
