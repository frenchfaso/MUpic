use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let author = fs::read(root.join("src/author_dither.rs")).expect("Read algorithm source");
    let core = fs::read_to_string(root.join("src/core.rs")).expect("Read palette source");
    let begin = core.find("pub const PALETTE:").expect("Locate palette");
    let end = begin + core[begin..].find("\n];").expect("Locate palette end") + 3;
    let mut portable = Sha256::new();
    portable.update(&author);
    portable.update(core[begin..end].as_bytes());
    let asset_id = portable
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    println!("cargo:rustc-env=MUP_LUT_ID={asset_id}");
    let mut hash = Sha256::new();
    for file in [
        "build.rs",
        "src/author_dither.rs",
        "src/core.rs",
        "src/lut.rs",
        "Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={file}");
        hash.update(file.as_bytes());
        hash.update([0]);
        hash.update(fs::read(root.join(file)).expect("Read LUT identity source"));
    }
    for name in [
        "TARGET",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_CFG_PANIC",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTFLAGS",
        "MACOSX_DEPLOYMENT_TARGET",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(env::var(name).unwrap_or_default().as_bytes());
        hash.update([0]);
    }
    let compiler = Command::new(env::var_os("RUSTC").unwrap())
        .arg("-vV")
        .output()
        .expect("Read Rust compiler identity");
    assert!(compiler.status.success(), "Read Rust compiler identity");
    hash.update(compiler.stdout);
    let digest = hash.finalize();
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    println!("cargo:rustc-env=MUP_LUT_BLOCK_ID={hex}");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("lut_id.rs"),
        format!("const IDENTITY: [u8; 32] = {:?};\n", digest.as_slice()),
    )
    .expect("Write LUT identity");
    let asset = root.parent().unwrap().join("site/assets/rgb24-lut.bin.gz");
    println!("cargo:rerun-if-changed={}", asset.display());
    let embedded = if !env::var("TARGET").unwrap().starts_with("wasm32") && asset.is_file() {
        format!(
            "pub const EMBEDDED_LUT: Option<&'static [u8]> = Some(include_bytes!({:?}));\n",
            asset.to_string_lossy()
        )
    } else {
        "pub const EMBEDDED_LUT: Option<&'static [u8]> = None;\n".to_owned()
    };
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("embedded_lut.rs"),
        embedded,
    )
    .expect("Write embedded LUT descriptor");
}
