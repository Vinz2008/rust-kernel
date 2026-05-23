use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=initrd");
    let _ = Command::new("tar")
        .args([
            "-C",
            "initrd",
            "-cf",
            "initrd.tar",
            ".",
        ])
        .status()
        .expect("failed to execute tar");
}