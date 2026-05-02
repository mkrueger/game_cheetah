use {
    std::{env, io},
    winres::WindowsResource,
};

fn main() -> io::Result<()> {
    // Enable AVX2 if available
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "x86_64" {
        println!("cargo:rustc-env=RUSTFLAGS=-C target-feature=+avx2");
    }

    // Compile Windows resources if on Windows
    // This will embed an icon into the executable.
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = WindowsResource::new();
        res.set_icon("build/windows/app.ico");
        // When cross-compiling from Linux, use the mingw-prefixed windres.
        let target = env::var("TARGET").unwrap_or_default();
        let host = env::var("HOST").unwrap_or_default();
        if !host.contains("windows") && target.contains("x86_64-pc-windows-gnu") {
            res.set_windres_path("x86_64-w64-mingw32-windres");
            res.set_ar_path("x86_64-w64-mingw32-ar");
        }
        res.compile()?;
    }
    Ok(())
}
