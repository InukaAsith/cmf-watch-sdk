//! The built-in site logos (scripts/browser-logos → logos.local.bin, not committed): into the app when there, else none.
fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("logos.bin");
    let src = std::path::Path::new("logos.local.bin");
    println!("cargo:rerun-if-changed=logos.local.bin");
    let data = std::fs::read(src).unwrap_or_else(|_| vec![0]);
    std::fs::write(out, data).unwrap();
}
