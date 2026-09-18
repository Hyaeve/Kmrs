//! xtask: engineering helper commands.
//! - `cargo xtask sync-migrations`: reconcile the migration files embedded in komga-rs with the Java Flyway source files, byte for byte.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("sync-migrations") => sync_migrations(),
        Some("dump-checksums") => dump_checksums(),
        Some("dump-schema") => dump_schema(),
        other => bail!(
            "unknown command {other:?}; available: sync-migrations, dump-checksums, dump-schema"
        ),
    }
}

/// Root of the komga source checkout: from `KOMGA_REPO_DIR`, otherwise the `komga` directory next to this repo.
fn komga_repo_dir() -> Result<PathBuf> {
  if let Ok(dir) = std::env::var("KOMGA_REPO_DIR") {
    let dir = PathBuf::from(dir);
    if dir.join("komga/src/flyway").exists() {
      return Ok(dir);
    }
    bail!("KOMGA_REPO_DIR={} does not contain komga/src/flyway", dir.display());
  }
  let sibling = Path::new(env!("CARGO_MANIFEST_DIR"))
    .parent()
    .and_then(Path::parent)
    .map(|p| p.join("komga"))
    .filter(|p| p.join("komga/src/flyway").exists());
  sibling.ok_or_else(|| {
    anyhow::anyhow!("komga source checkout not found; set KOMGA_REPO_DIR or place komga next to this repo")
  })
}

/// Root of this repo (komga-rs).
fn repo_root() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR"))
    .parent()
    .expect("xtask is at <repo>/xtask")
    .to_path_buf()
}

fn read_dir_map(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut map = BTreeMap::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().into_string().unwrap();
        if name.ends_with(".sql") {
            map.insert(name, fs::read(entry.path())?);
        }
    }
    Ok(map)
}

fn sync_migrations() -> Result<()> {
    let komga_repo = komga_repo_dir()?;
    let root = repo_root();
    let mut failed = false;
    for (java_dir, rust_dir) in [
        (
            komga_repo.join("komga/src/flyway/resources/db/migration/sqlite"),
            root.join("crates/komga-db/migrations"),
        ),
        (
            komga_repo.join("komga/src/flyway/resources/tasks/migration/sqlite"),
            root.join("crates/komga-db/migrations_tasks"),
        ),
    ] {
        let java = read_dir_map(&java_dir)?;
        let rust = read_dir_map(&rust_dir)?;
        for name in java
            .keys()
            .chain(rust.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            match (java.get(name), rust.get(name)) {
                (Some(j), Some(r)) if j == r => {}
                (Some(_), Some(_)) => {
                    eprintln!(
                        "DIFF  {name} ({} vs {})",
                        java_dir.display(),
                        rust_dir.display()
                    );
                    failed = true;
                }
                (Some(_), None) => {
                    eprintln!("MISSING in rust: {name}");
                    failed = true;
                }
                (None, Some(_)) => {
                    eprintln!("EXTRA in rust: {name}");
                    failed = true;
                }
                (None, None) => unreachable!(),
            }
        }
        println!(
            "{}: {} files checked",
            rust_dir.display(),
            rust.len().max(java.len())
        );
    }
    if failed {
        bail!("migration files out of sync; copy from komga/src/flyway/resources/");
    }
    println!("migrations in sync");
    Ok(())
}

/// Print `<file> <crc32>` for all main-db migrations (default placeholders), for cross-checking against an independent implementation.
fn dump_checksums() -> Result<()> {
    let placeholders = komga_db::Placeholders::default();
    for m in komga_db::main_migrations() {
        if let komga_db::migrate::Migration::Sql(sql) = m {
            let resolved = placeholders.substitute(sql.sql);
            let checksum = komga_db::migrate::flyway_checksum(&resolved);
            println!("{} {}", sql.file_name, checksum);
        }
    }
    Ok(())
}

/// Run all migrations on a temporary database and print the sqlite_master schema (DDL reference).
fn dump_schema() -> Result<()> {
    use rusqlite::Connection;
    let conn = Connection::open_in_memory()?;
    let migrations = komga_db::main_migrations();
    komga_db::Migrator::new(&migrations, komga_db::Placeholders::default()).migrate(&conn)?;
    let mut stmt = conn.prepare(
    "SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' AND name <> 'flyway_schema_history' ORDER BY type, name",
  )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for row in rows {
        println!("{};\n", row?);
    }
    Ok(())
}
