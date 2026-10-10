#![cfg(windows)]

use std::fs;

use kiln::search::run_pagefind;

// ── run_pagefind ──

#[test]
#[ignore = "requires globally installed npm Pagefind"]
fn run_pagefind_npm_indexes_site() {
    let dir = tempfile::Builder::new()
        .prefix("pagefind site & (test) ")
        .tempdir()
        .unwrap();
    fs::write(
        dir.path().join("index.html"),
        r#"<html lang="en"><body><main data-pagefind-body>Searchable example</main></body></html>"#,
    )
    .unwrap();

    run_pagefind(dir.path(), None).unwrap();

    assert!(dir.path().join("pagefind/pagefind.js").is_file());
    let entry: serde_yaml::Value =
        serde_yaml::from_slice(&fs::read(dir.path().join("pagefind/pagefind-entry.json")).unwrap())
            .unwrap();
    assert_eq!(entry["languages"]["en"]["page_count"].as_u64(), Some(1));
}
