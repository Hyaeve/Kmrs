//! Build-time metadata for the actuator `/actuator/info` git section.

fn main() {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    println!(
        "cargo:rustc-env=GIT_BRANCH={}",
        git(&["rev-parse", "--abbrev-ref", "HEAD"])
    );
    println!(
        "cargo:rustc-env=GIT_COMMIT_ID={}",
        git(&["rev-parse", "--short", "HEAD"])
    );
    println!(
        "cargo:rustc-env=GIT_COMMIT_TIME={}",
        git(&["log", "-1", "--format=%cI"])
    );
    println!("cargo:rerun-if-changed=.git/HEAD");
}
