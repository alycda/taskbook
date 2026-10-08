//! Build script for the optional `ditto` feature: finds (or downloads) the
//! prebuilt Ditto native library and emits the link directives.
//!
//! Resolution order for `libdittoffi.a`:
//! 1. `DITTO_SDK_DIR` — a directory containing the library.
//! 2. `DITTOFFI_SEARCH_PATH` — same, kept for parity with the official crate.
//! 3. The download cache: `DITTO_SDK_CACHE`, else
//!    `$XDG_CACHE_HOME/taskbook/ditto-sdk` (`~/.cache/...`), else `OUT_DIR`.
//!    It is keyed by SDK version and target so check/build/test and
//!    different toolchains share one copy.
//! 4. Download into that cache from `software.ditto.live` with `curl`,
//!    unless `DITTO_LOCAL_BUILD=1`, which forbids network access (Nix,
//!    offline CI).
//!
//! The library is the per-target artifact Ditto publishes for its Rust SDK;
//! it carries the same `dittoffi` C ABI as the C++ SDK's `libditto.a`, minus
//! the C++ wrapper (whose bundled Rust runtime collides with ours).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Ditto SDK release the FFI declarations in `src/storage/ditto/ffi.rs` were
/// transcribed from. Bumping it means re-checking that file (see
/// `scripts/ditto-abi-check.sh`).
const DITTO_SDK_VERSION: &str = "4.14.7";
const LIB_NAME: &str = "dittoffi";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=TB_DITTO_SDK_VERSION={DITTO_SDK_VERSION}");

    if env::var_os("CARGO_FEATURE_DITTO").is_none() {
        return;
    }

    for var in [
        "DITTO_SDK_DIR",
        "DITTOFFI_SEARCH_PATH",
        "DITTO_SDK_CACHE",
        "DITTO_LOCAL_BUILD",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    let target = env::var("TARGET").expect("TARGET is set by cargo");
    let lib_file = lib_filename(&target);

    let lib_dir = find_or_fetch(&target, &lib_file);

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static={LIB_NAME}");

    // Platform libraries the static archive relies on (mirrors the official
    // crate's build script).
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=framework=SystemConfiguration");
    } else if target.contains("windows") {
        for lib in ["ntdll", "iphlpapi", "oleaut32", "ole32"] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}

fn lib_filename(target: &str) -> String {
    if target.contains("windows-msvc") {
        format!("{LIB_NAME}.lib")
    } else {
        format!("lib{LIB_NAME}.a")
    }
}

fn find_or_fetch(target: &str, lib_file: &str) -> PathBuf {
    for var in ["DITTO_SDK_DIR", "DITTOFFI_SEARCH_PATH"] {
        if let Some(dir) = env::var_os(var).map(PathBuf::from) {
            let candidate = dir.join(lib_file);
            if candidate.is_file() {
                return dir;
            }
            panic!("{var}={} does not contain {lib_file}", dir.display());
        }
    }

    let cache_dir = cache_root().join(DITTO_SDK_VERSION).join(target);
    let cached = cache_dir.join(lib_file);
    if cached.is_file() {
        return cache_dir;
    }

    if env::var("DITTO_LOCAL_BUILD")
        .map(|v| v != "0")
        .unwrap_or(false)
    {
        panic!(
            "DITTO_LOCAL_BUILD is set and {lib_file} was not found; point DITTO_SDK_DIR at a \
             directory containing the Ditto {DITTO_SDK_VERSION} library for {target}"
        );
    }

    fs::create_dir_all(&cache_dir).expect("create cache dir");
    download(target, lib_file, &cached);
    cache_dir
}

/// Where downloaded libraries are kept across builds.
fn cache_root() -> PathBuf {
    if let Some(dir) = env::var_os("DITTO_SDK_CACHE") {
        return PathBuf::from(dir);
    }
    let base = env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .or_else(|| env::var_os("LOCALAPPDATA").map(PathBuf::from));
    match base {
        Some(base) => base.join("taskbook").join("ditto-sdk"),
        None => {
            PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo")).join("ditto-sdk")
        }
    }
}

fn download(target: &str, lib_file: &str, dest: &Path) {
    let url = format!(
        "https://software.ditto.live/rust/Ditto/{DITTO_SDK_VERSION}/{target}/release/{lib_file}"
    );
    println!("cargo:warning=downloading Ditto native library {url}");

    let tmp = dest.with_extension("part");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--output",
        ])
        .arg(&tmp)
        .arg(&url)
        .status()
        .unwrap_or_else(|e| {
            panic!("could not run curl to fetch the Ditto library ({e}); set DITTO_SDK_DIR instead")
        });
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        panic!("downloading {url} failed ({status}); set DITTO_SDK_DIR to a local copy");
    }
    let size = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if size < 1024 * 1024 {
        let _ = fs::remove_file(&tmp);
        panic!("downloaded {url} is implausibly small ({size} bytes)");
    }
    fs::rename(&tmp, dest).expect("move downloaded library into place");
}
