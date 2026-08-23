fn main() {
    println!("cargo:rerun-if-changed=assets/icon/vibez.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/icon/vibez.ico")
        .set("FileDescription", "vibez DAW")
        .set("ProductName", "vibez")
        .set("OriginalFilename", "vibez.exe");
    resource
        .compile()
        .expect("failed to embed the vibez Windows executable resources");
}
