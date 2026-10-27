//! Windows: give trazo.exe its icon and version details (Explorer, taskbar,
//! shortcuts, "Aplicaciones instaladas").

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/trazo.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/trazo.ico");
        res.set("ProductName", "Trazo");
        res.set("FileDescription", "Trazo");
        res.set("CompanyName", "Inerza Dynamics");
        res.set("LegalCopyright", "GPL-3.0-or-later");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no se pudo poner el icono al exe: {e}");
        }
    }
}
