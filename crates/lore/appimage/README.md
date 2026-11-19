# The AppImage runtime

`runtime-x86_64` is the program at the front of every AppImage `lore package --appimage` writes (#141): it mounts the game's files, which follow it in the same file, and starts the game. It is AppImage's own type 2 runtime, unchanged:

- Source: <https://github.com/AppImage/type2-runtime>, release `20251108`
- Downloaded from <https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64>
- SHA-256 `2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d`, 944 632 bytes
- Licence: MIT, in `LICENSE` beside it, which every AppImage carries in its `licenses/` folder

It is kept here, not downloaded when a game is packaged, so packaging works with no network, exactly as the `.tar.gz` does. It is statically linked and needs no libfuse on the player's computer, only the kernel's FUSE and `fusermount`/`fusermount3`.

To update it, download a newer pinned release (never the moving `continuous` tag), check its signature against the project's `signing-pubkey.asc`, replace the file, and update the release, hash and size here; the tests in `crates/lore/src/package.rs` check that the game's files start where the runtime looks for them.
