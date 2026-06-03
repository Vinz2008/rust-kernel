use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=../initrd");

    let profile = env::var("PROFILE").unwrap();

    let init_bin = PathBuf::from(format!(
        "../target/x86_64-rust_kernel/{profile}/init"
    ));



    dbg!(init_bin.display());
    fs::copy(&init_bin, "../initrd/init").expect("failed to copy init executable");
    

    let _ = Command::new("tar")
        .args([
            "-C",
            "../initrd",
            "-cf",
            "initrd.tar",
            ".",
        ])
        .status()
        .expect("failed to execute tar");
}