fn main() {
    #[cfg(target_os = "windows")]
    {
        let icon_path = std::path::Path::new("resources/windows/app.ico");
        let rc_path = std::path::Path::new("resources/windows/app.rc");

        if icon_path.exists() && rc_path.exists() {
            println!("cargo:rerun-if-changed={}", icon_path.display());
            println!("cargo:rerun-if-changed={}", rc_path.display());

            embed_resource::compile(rc_path, embed_resource::NONE)
                .manifest_optional()
                .unwrap();
        } else {
            println!(
                "cargo:warning=No Windows icon found at {}; the application will use the default icon.",
                icon_path.display()
            );
        }
    }
}
