//! Architectural guard: the dependency layering from CLAUDE.md / ARCHITECTURE.md
//! is a hard rule, but nothing enforces it at build time and the project has no
//! CI. This test parses every crate manifest and asserts that each internal
//! (`fah-*`) dependency points to a strictly lower layer — so a stray sibling
//! import (e.g. `fah-dns` depending on `fah-api`) fails `cargo test`.

use std::fs;
use std::path::PathBuf;

/// Layer index per crate (L1 lowest). Dependencies may only point to a strictly
/// lower layer; siblings (same layer) must never import each other.
fn layer(crate_name: &str) -> Option<u8> {
    Some(match crate_name {
        "fah-model" | "fah-config" | "fah-common" | "fah-logging" => 1,
        "fah-rules" => 2,
        "fah-dns" | "fah-api" | "fah-stats" | "fah-metrics" => 3,
        "fastadhunter" => 4,
        _ => return None,
    })
}

fn crates_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/fastadhunter; its parent is crates/.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/fastadhunter has a parent")
        .to_path_buf()
}

#[test]
fn internal_dependencies_point_strictly_downward() {
    let mut checked = 0;

    for entry in fs::read_dir(crates_dir()).expect("read crates/") {
        let manifest = entry.expect("dir entry").path().join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }

        let text = fs::read_to_string(&manifest).expect("read Cargo.toml");
        let doc: toml::Value = toml::from_str(&text).expect("parse Cargo.toml");
        let name = doc["package"]["name"]
            .as_str()
            .expect("package.name")
            .to_string();
        let this =
            layer(&name).unwrap_or_else(|| panic!("unknown crate `{name}` — add it to layer()"));

        for table in ["dependencies", "dev-dependencies"] {
            let Some(deps) = doc.get(table).and_then(toml::Value::as_table) else {
                continue;
            };
            for dep in deps.keys().filter(|k| k.starts_with("fah-")) {
                let dep_layer =
                    layer(dep).unwrap_or_else(|| panic!("{name} depends on unknown crate `{dep}`"));
                assert!(
                    dep_layer < this,
                    "layering violation: {name} (L{this}) depends on {dep} (L{dep_layer}); \
                     dependencies must point to a strictly lower layer"
                );
            }
        }

        checked += 1;
    }

    assert_eq!(
        checked, 10,
        "expected 10 workspace crates, checked {checked}"
    );
}
