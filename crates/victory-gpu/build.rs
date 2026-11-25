//! The shader cache's key for this build's translator (`src/shader_cache.rs`):
//! a hash of the translator's sources (victory-shader, the cache's own
//! encoding, the lock file), and the `VICTORY_*` switches the translator
//! reads, so that a rebuild that leaves the translator alone keeps the
//! translations cached by the build before.

use std::path::{Path, PathBuf};

/// FNV-1a over 128 bits: stable across compilers, which `std`'s hasher is
/// not promised to be.
struct Fnv(u128);

impl Fnv {
    fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u128::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013B);
        }
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let shader = here.join("../victory-shader");
    let mut inputs = Vec::new();
    files(&shader.join("src"), &mut inputs);
    inputs.push(shader.join("Cargo.toml"));
    inputs.push(here.join("src/shader_cache.rs"));
    inputs.push(here.join("../../Cargo.lock"));
    inputs.sort();
    let mut hash = Fnv(0x6C62_272E_07BB_0142_62B8_2175_6295_C58D);
    let mut switches = std::collections::BTreeSet::new();
    for path in &inputs {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = path.strip_prefix(&here).unwrap_or(path).to_string_lossy().replace('\\', "/");
        hash.update(name.as_bytes());
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
        if path.extension().is_some_and(|e| e == "rs") && path.starts_with(shader.join("src")) {
            let text = String::from_utf8_lossy(&bytes);
            let mut rest = &text[..];
            while let Some(at) = rest.find("\"VICTORY_") {
                let tail = &rest[at + 1..];
                let end = tail.find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')).unwrap_or(tail.len());
                switches.insert(tail[..end].to_owned());
                rest = &tail[end..];
            }
        }
    }
    println!("cargo:rerun-if-changed={}", shader.join("src").display());
    println!("cargo:rerun-if-changed={}", shader.join("Cargo.toml").display());
    println!("cargo:rerun-if-changed={}", here.join("src/shader_cache.rs").display());
    println!("cargo:rerun-if-changed={}", here.join("../../Cargo.lock").display());
    let list: Vec<String> = switches.iter().map(|s| format!("{s:?}")).collect();
    let code = format!(
        "/// A hash of the translator's sources (see build.rs).\npub(crate) const SHADER_SOURCE: u128 = {:#x};\n\
         /// The switches the translator reads.\npub(crate) const SHADER_SWITCHES: &[&str] = &[{}];\n",
        hash.0,
        list.join(", ")
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("shader_source.rs");
    std::fs::write(out, code).unwrap();
}
