//! 把 `assets/icons` 下的 SVG 编译进二进制。
//!
//! Android APK 里没有桌面端的 `assets/icons` 目录；图标必须随 `.so` 一起分发。
//! 桌面端仍优先从磁盘读取（方便替换图标），这里生成的表只作为兜底。

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/icons");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let icon_dir = Path::new(&manifest_dir).join("assets/icons");
    let mut icons: Vec<(String, String)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&icon_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("svg") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            let path = path.to_string_lossy().replace('\\', "/");
            icons.push((name.to_owned(), path));
        }
    }
    icons.sort_by(|left, right| left.0.cmp(&right.0));

    let mut generated = String::from("static EMBEDDED_ICONS: &[(&str, &[u8])] = &[\n");
    for (name, path) in icons {
        generated.push_str(&format!(
            "    (\"md3-icons/{name}.svg\", include_bytes!({path:?})),\n"
        ));
    }
    generated.push_str("];\n");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&out_dir).join("embedded_icons.rs"), generated)
        .expect("write embedded_icons.rs");
}
