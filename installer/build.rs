fn main() {
    println!("cargo:rerun-if-changed=../assets/branding/framely.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../assets/branding/framely.ico")
            .set("ProductName", "Framely Installer")
            .set("FileDescription", "Framely Installer")
            .compile()
            .expect("Could not embed Framely Windows application icon");
    }
}
