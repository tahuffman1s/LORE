//! LORE is single-player only: there is no network code, no server and no
//! second player, and a game played alone opens no port. This reads the
//! engine's own source and fails if any of it reaches for a socket, so
//! none comes back unnoticed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

/// What a file would have to name to listen or to connect.
const SOCKETS: [&str; 7] = [
    "std::net",
    "TcpListener",
    "TcpStream",
    "UdpSocket",
    "tokio::net",
    "mio::net",
    "socket2",
];

/// Every Rust file under `folder`.
fn sources(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn nothing_in_the_engine_opens_a_socket() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    sources(crates, &mut files);
    assert!(files.len() > 100, "found only {} files", files.len());
    let this = Path::new(file!()).file_name().unwrap();
    let mut reaching = Vec::new();
    for file in &files {
        // This file names them to look for them.
        if file.file_name() == Some(this) {
            continue;
        }
        let text = fs::read_to_string(file).unwrap();
        for socket in SOCKETS {
            if text.contains(socket) {
                reaching.push(format!("{} names {socket}", file.display()));
            }
        }
    }
    assert!(
        reaching.is_empty(),
        "LORE has no network code (CLAUDE.md, Decisions), but:\n{}",
        reaching.join("\n")
    );
}

/// And none of the crates is built with a networking library.
#[test]
fn no_crate_depends_on_a_networking_library() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for entry in fs::read_dir(crates).unwrap() {
        let manifest = entry.unwrap().path().join("Cargo.toml");
        let Ok(text) = fs::read_to_string(&manifest) else {
            continue;
        };
        for library in [
            "tokio", "hyper", "reqwest", "quinn", "laminar", "renet", "socket2",
        ] {
            let named = text
                .lines()
                .any(|line| line.trim_start().starts_with(&format!("{library} ")));
            assert!(!named, "{} depends on {library}", manifest.display());
        }
    }
}
