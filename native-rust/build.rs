use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    for input in ["buddy.rc", "buddy.ico", "buddy.manifest"] {
        println!("cargo:rerun-if-changed={input}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let version = env!("CARGO_PKG_VERSION");
    let (base, revision) = version.split_once("-beta.").unwrap_or((version, "65535"));
    let numbers = format!("{},{}", base.replace('.', ","), revision);
    fs::write(
        dir.join("buddy-version.h"),
        format!("#define BUDDY_VERSION_NUM {numbers}\n#define BUDDY_VERSION_STR \"{version}\"\n"),
    ).expect("write Buddy's Windows version");
    let out = dir.join("buddy.res");
    let status = Command::new("rc.exe")
        .arg("/nologo").arg("/i").arg(&dir)
        .arg(format!("/fo{}", out.display())).arg("buddy.rc")
        .status().expect("Windows SDK resource compiler");
    assert!(status.success(), "Could not compile Windows resources.");
    println!("cargo:rustc-link-arg-bin=BuddyRust={}", out.display());
    println!("cargo:rustc-link-arg-bin=BuddyRust=/MANIFEST:NO");
}
