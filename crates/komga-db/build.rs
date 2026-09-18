//! Registers the SQL files under migrations/ and migrations_tasks/ as a static
//! migration list at build time.
//! Migration files are byte-for-byte copies of komga's Flyway migrations,
//! reconciled by `cargo xtask sync-migrations`.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    for (dir, out_name) in [
        ("migrations", "migrations_main.rs"),
        ("migrations_tasks", "migrations_tasks.rs"),
    ] {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("cannot read {dir}: {e}"))
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".sql"))
            .collect();
        names.sort();
        let mut out = String::from("&[\n");
        for name in &names {
            out.push_str(&format!(
        "crate::migrate::SqlMigration {{ file_name: {name:?}, sql: include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{dir}/{name}\")) }},\n"
      ));
        }
        out.push_str("]\n");
        fs::write(Path::new(&out_dir).join(out_name), out).unwrap();
        println!("cargo:rerun-if-changed={dir}");
    }
}
