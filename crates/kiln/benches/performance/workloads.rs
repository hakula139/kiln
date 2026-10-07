use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

#[derive(Serialize)]
struct Workload<'a> {
    id: &'a str,
    contract: &'a str,
    inputs: String,
}

/// Change the contract version when the timed operation or excluded setup changes.
pub(super) fn record(id: &str, contract: &str, inputs: &[&[u8]]) {
    let Some(path) = std::env::var_os("KILN_BENCH_MANIFEST") else {
        return;
    };

    let workload = Workload {
        id,
        contract,
        inputs: fingerprint(inputs),
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "[[workloads]]").unwrap();
    writeln!(file, "{}", toml::to_string(&workload).unwrap()).unwrap();
}

fn fingerprint(inputs: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for input in inputs {
        digest.update(u64::try_from(input.len()).unwrap().to_le_bytes());
        digest.update(input);
    }
    hex::encode(digest.finalize())
}

pub(super) fn site_inputs(root: &Path) -> String {
    let mut files: Vec<_> = WalkDir::new(root)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect();
    files.sort();
    let contents: Vec<_> = files
        .iter()
        .flat_map(|path| {
            [
                path.strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .as_bytes()
                    .to_vec(),
                fs::read(path).unwrap(),
            ]
        })
        .collect();
    fingerprint(&contents.iter().map(Vec::as_slice).collect::<Vec<_>>())
}
