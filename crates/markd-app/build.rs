// Windows 下把应用图标嵌入 EXE 资源（.rc）：
// 没有这一步，Cargo 产出的 markd.exe 在资源管理器 / 桌面快捷方式上是默认图标。
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../assets/icon.ico");
        winresource::WindowsResource::new()
            .set_icon(icon.to_str().unwrap())
            .compile()
            .expect("failed to embed Windows icon resource");
    }
}
