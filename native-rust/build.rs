use std::{env,path::PathBuf,process::Command};
fn main(){
    println!("cargo:rerun-if-changed=buddy.rc");println!("cargo:rerun-if-changed=buddy.ico");println!("cargo:rerun-if-changed=buddy.manifest");
    if env::var("CARGO_CFG_TARGET_OS").as_deref()!=Ok("windows"){return}
    let out=PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("buddy.res");
    let status=Command::new("rc.exe").arg("/nologo").arg(format!("/fo{}",out.display())).arg("buddy.rc").status().expect("Windows SDK resource compiler");
    assert!(status.success(),"Could not compile Windows resources.");
    println!("cargo:rustc-link-arg-bin=BuddyRust={}",out.display());
    println!("cargo:rustc-link-arg-bin=BuddyRust=/MANIFEST:NO");
}
