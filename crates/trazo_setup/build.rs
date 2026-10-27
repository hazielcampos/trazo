//! The installer carries the Trazo exe inside it. By default it takes
//! `target/release/trazo.exe` (build that first); `TRAZO_SETUP_PAYLOAD` can
//! point somewhere else. Without one, the installer is built empty and says so.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let payload = std::env::var_os("TRAZO_SETUP_PAYLOAD")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../target/release/trazo.exe"));
    println!("cargo:rerun-if-env-changed=TRAZO_SETUP_PAYLOAD");
    println!("cargo:rerun-if-changed={}", payload.display());
    println!("cargo:rerun-if-changed=../../assets/icon/trazo.ico");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.bin");
    if payload.exists() {
        std::fs::copy(&payload, &out).expect("copy the Trazo exe into the installer");
    } else {
        std::fs::write(&out, []).unwrap();
        println!("cargo:warning=instalador sin Trazo dentro: compila antes `cargo build --release -p trazo_app`");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/trazo.ico");
        res.set("ProductName", "Trazo");
        res.set("FileDescription", "Instalador de Trazo");
        res.set("CompanyName", "Inerza Dynamics");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no se pudo poner el icono al instalador: {e}");
        }
    }
}
