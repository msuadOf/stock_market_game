fn main() {
    #[cfg(windows)]
    {
        // Tauri's default resource links only to bins. Embed the same manifest
        // into every artifact so library tests can load TaskDialogIndirect too.
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("failed to build Tauri resources");
        println!("cargo:rerun-if-changed=windows-manifest.rc");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        embed_resource::compile_for_everything("windows-manifest.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed Windows Common Controls v6 manifest");
    }
    #[cfg(not(windows))]
    tauri_build::build();
}
