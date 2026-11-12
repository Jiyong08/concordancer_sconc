fn main() {
    #[cfg(target_os = "windows")]
    {
        if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../assets/icon.ico");
            if let Err(e) = res.compile() {
                println!("cargo:warning=Failed to embed Windows icon: {}", e);
            }
        }
    }
}
