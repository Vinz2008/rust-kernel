use std::{env, fs, path::PathBuf, process::Command};

fn add_exe_to_initrd(exe : &str, profile : &str){
    let bin = PathBuf::from(format!(
        "../target/x86_64-unknown-rust_kernel/{profile}/{exe}"
    ));

    dbg!(bin.display());
    let to_path = format!("../initrd/{exe}");
    fs::copy(&bin, to_path).expect("failed to copy executable");
}

fn main() {
    println!("cargo:rustc-link-arg=-Tkernel/linker.ld");
    println!("cargo:rerun-if-changed=../initrd");

    let profile = env::var("PROFILE").unwrap();

    add_exe_to_initrd("init", &profile);
    add_exe_to_initrd("cli", &profile);
    

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