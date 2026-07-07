use std::process::Command;

fn main() {
    // Capture the short commit hash at build time for the About screen. Falls
    // back to "unknown" outside a git checkout (e.g. a source tarball build).
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|hash| hash.trim().to_owned())
        .filter(|hash| !hash.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=NEXO_GIT_COMMIT={commit}");

    // Debug vs release build type.
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=NEXO_BUILD_PROFILE={profile}");

    // Rebuild when HEAD moves so the hash stays current.
    println!("cargo:rerun-if-changed=../../../.git/HEAD");

    tauri_build::build();
}
