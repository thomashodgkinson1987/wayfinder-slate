// rust-embed requires its source folder to exist at compile time. A fresh
// checkout has no frontend build yet, so create the empty folder and let the
// server serve a helpful 503 until `npm --prefix frontend run build` is run.
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let dist = manifest.join("../../frontend/dist");
    std::fs::create_dir_all(&dist).expect("create frontend/dist");
    println!("cargo:rerun-if-changed=build.rs");
    // rust-embed bakes the frontend into the binary at compile time. Without
    // this, `cargo build` can reuse a stale binary after `npm run build`.
    println!("cargo:rerun-if-changed=../../frontend/dist");
}
