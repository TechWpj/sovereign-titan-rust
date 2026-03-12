fn main() {
    tauri_build::build();

    // Link LLVM OpenMP runtime for clang-cl (ROCm) on Windows.
    // The llama-cpp-sys-2 build script enables OpenMP but only links gomp on Linux.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-search=native=C:/Program Files/LLVM/lib");
        println!("cargo:rustc-link-lib=dylib=libomp");
    }
}
