//! Gives softcaller.exe its icon and version information on Windows, for
//! Explorer, the taskbar and the installers' shortcuts (`softcaller.exe,0`).

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/softcaller.ico");
    // The target, not the host this build script runs on.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../packaging/icons/softcaller.ico").set("ProductName", "Softcaller").set("FileDescription", "Softcaller");
        resource.compile().expect("compiling the Windows resources (needs the Windows SDK's rc.exe)");
    }
}
