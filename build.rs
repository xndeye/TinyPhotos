fn main() {
    println!("cargo:rerun-if-changed=assets/tinyphotos.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").expect("无法读取目标平台") == "windows" {
        winresource::WindowsResource::new()
            .set_icon("assets/tinyphotos.ico")
            .compile()
            .expect("无法将程序图标嵌入 Windows 可执行文件");
    }
}
