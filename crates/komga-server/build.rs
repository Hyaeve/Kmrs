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

    // Explorer, taskbar and tray icons all come from the exe's embedded resources;
    // without this kmrs.exe shows the stock application icon. manifest_optional
    // skips hosts without a resource compiler (local cross `cargo check`) instead
    // of failing the build over a cosmetic asset; windows-latest CI has rc.exe,
    // so a genuine compile error there still fails the release build.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=resources/windows/kmrs.rc");
        println!("cargo:rerun-if-changed=resources/windows/kmrs.ico");
        embed_resource::compile("resources/windows/kmrs.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }

    // webui.rs embeds ../../webui/dist into the binary. A fresh checkout has no such
    // directory (it is vite's output), so substitute a placeholder page: the crate must
    // compile for builders who never touch the UI.
    let dist = std::path::Path::new("../../webui/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.html").is_file() {
        std::fs::create_dir_all(dist.join("assets")).unwrap();
        std::fs::write(
            dist.join("index.html"),
            "<html><body><p>The web UI is not bundled in this build. Build it with \
             <code>pnpm build</code> in <code>webui/</code>, then rebuild kmrs.</p></body></html>",
        )
        .unwrap();
        std::fs::write(dist.join("assets/placeholder.js"), "// no web UI bundled\n").unwrap();
    }
}
