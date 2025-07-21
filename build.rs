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
        WindowsResource::new()
            // This path can be absolute, or relative to your crate root.
            .set_icon("build/windows/app.ico")
            .compile()?;
    }
    Ok(())
}
