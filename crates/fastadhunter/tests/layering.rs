use std::fs;
use std::path::{Path, PathBuf};

const INTERNAL_PREFIX: &str = "fah-";
const TUI_MONITOR: &str = "fah-tui-monitor";
const TUI_MONITOR_ALLOWED: &[&str] = &["fah-model"];
const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

fn layer(crate_name: &str) -> Option<u8> {
    Some(match crate_name {
        "fah-model" | "fah-config" | "fah-common" | "fah-logging" => 1,
        "fah-rules" => 2,
        "fah-dns" | "fah-http" | "fah-api" | "fah-stats" | "fah-metrics" => 3,
        "fastadhunter" => 4,
        _ => return None,
    })
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/fastadhunter sits two levels below the workspace root")
        .to_path_buf()
}

fn read_manifest(path: &Path) -> toml::Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    toml::from_str(&text).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn members(root: &Path) -> Vec<PathBuf> {
    let doc = read_manifest(&root.join("Cargo.toml"));
    let patterns = doc["workspace"]["members"]
        .as_array()
        .expect("[workspace] members is an array");

    let mut members = Vec::new();
    for pattern in patterns {
        let pattern = pattern.as_str().expect("workspace member is a string");
        match pattern.strip_suffix("/*") {
            Some(dir) => {
                let dir = root.join(dir);
                let entries = fs::read_dir(&dir)
                    .unwrap_or_else(|err| panic!("read member glob {}: {err}", dir.display()));
                for entry in entries {
                    let path = entry.expect("dir entry").path();
                    if path.is_dir() {
                        members.push(path);
                    }
                }
            }
            None => {
                assert!(
                    !pattern.contains('*'),
                    "unsupported member glob `{pattern}`"
                );
                members.push(root.join(pattern));
            }
        }
    }

    for member in &members {
        assert!(
            member.join("Cargo.toml").is_file(),
            "workspace member {} has no Cargo.toml",
            member.display()
        );
    }
    members
}

fn real_name<'a>(key: &'a str, value: &'a toml::Value) -> &'a str {
    value
        .get("package")
        .and_then(toml::Value::as_str)
        .unwrap_or(key)
}

fn collect_internal(table: &toml::Value, out: &mut Vec<String>) {
    let Some(deps) = table.as_table() else {
        return;
    };
    for (key, value) in deps {
        let name = real_name(key, value);
        if name.starts_with(INTERNAL_PREFIX) {
            out.push(name.to_string());
        }
    }
}

fn shipping_deps(doc: &toml::Value) -> Vec<String> {
    let mut out = Vec::new();
    for table in ["dependencies", "build-dependencies"] {
        if let Some(deps) = doc.get(table) {
            collect_internal(deps, &mut out);
        }
    }
    if let Some(targets) = doc.get("target").and_then(toml::Value::as_table) {
        for cfg in targets.values() {
            for table in ["dependencies", "build-dependencies"] {
                if let Some(deps) = cfg.get(table) {
                    collect_internal(deps, &mut out);
                }
            }
        }
    }
    out
}

fn internal_deps(doc: &toml::Value) -> Vec<String> {
    let mut out = Vec::new();
    for table in DEP_TABLES {
        if let Some(deps) = doc.get(table) {
            collect_internal(deps, &mut out);
        }
    }
    if let Some(targets) = doc.get("target").and_then(toml::Value::as_table) {
        for cfg in targets.values() {
            for table in DEP_TABLES {
                if let Some(deps) = cfg.get(table) {
                    collect_internal(deps, &mut out);
                }
            }
        }
    }
    out
}

#[test]
fn internal_dependencies_point_strictly_downward() {
    let root = workspace_root();
    let members = members(&root);
    let mut checked = 0;
    let mut saw_tui_monitor = false;

    for member in &members {
        let doc = read_manifest(&member.join("Cargo.toml"));
        let name = doc["package"]["name"].as_str().expect("package.name");
        let deps = internal_deps(&doc);

        if name == TUI_MONITOR {
            saw_tui_monitor = true;
            for dep in &deps {
                assert!(
                    TUI_MONITOR_ALLOWED.contains(&dep.as_str()),
                    "layering violation: {name} depends on {dep}; \
                     its internal dependencies are limited to {TUI_MONITOR_ALLOWED:?}"
                );
            }
        } else {
            let this =
                layer(name).unwrap_or_else(|| panic!("unknown crate `{name}` — add it to layer()"));
            assert!(
                !shipping_deps(&doc).iter().any(|dep| dep == name),
                "layering violation: {name} depends on itself outside [dev-dependencies]; \
                 a self-edge is only ever a test-only feature switch"
            );
            for dep in deps.iter().filter(|dep| dep.as_str() != name) {
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

    assert!(saw_tui_monitor, "{TUI_MONITOR} is not a workspace member");
    assert_eq!(
        checked,
        members.len(),
        "every workspace member must be checked"
    );
    assert!(
        checked > 1,
        "workspace member discovery found only {checked} crate"
    );
}
