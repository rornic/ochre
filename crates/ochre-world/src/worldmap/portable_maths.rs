//! A seed makes the same world on every machine: no platform maths library in world generation,
//! and no dependence on how many threads it runs on.

use std::path::{Path, PathBuf};

use super::*;
use std::sync::atomic::Ordering;

// FNV-1a
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

// pinned: a change means every machine's map changed, so bump `worldmap::GENERATOR_VERSION` with it
const MAP_HASH: u64 = 9_980_198_464_709_189_974;

#[test]
fn test_the_map_is_the_same_on_every_machine_and_for_any_thread_count() {
    let mut seen = Vec::new();
    for threads in [1usize, 7] {
        THREADS_FOR_TEST.0.store(threads, Ordering::Relaxed);
        seen.push(hash(&generate(1, WorldConfig::default()).to_bytes()));
    }
    assert_eq!(seen[0], seen[1], "the map depends on the thread count");
    assert_eq!(MAP_HASH, seen[0], "map hash changed");
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn test_world_generation_calls_no_platform_maths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    sources(&root.join("ochre-base/src"), &mut files);
    sources(&root.join("ochre-world/src/worldmap"), &mut files);
    sources(&root.join("ochre-terrain/src/generate"), &mut files);
    let banned = [
        "exp", "exp2", "powf", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh",
        "cosh", "tanh", "hypot", "ln", "log2", "log10", "cbrt", "sin_cos",
    ];
    let mut found = Vec::new();
    for path in files {
        // render shading, not world content
        if path.ends_with("generator/greedy.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let live = text.split("#[cfg(test)]\nmod ").next().unwrap();
        for (n, line) in live.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            for name in banned {
                if code.contains(&format!(".{name}("))
                    || code.contains(&format!("::{name}(")) && !code.contains("libm::")
                {
                    found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "use the libm crate for:\n{}",
        found.join("\n")
    );
}
