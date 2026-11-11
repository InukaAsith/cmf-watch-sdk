use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("app.ld"), include_bytes!("app.ld")).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=app.ld");
    println!("cargo::rustc-check-cfg=cfg(wp2_sim)");
}
