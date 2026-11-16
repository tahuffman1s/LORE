#!/usr/bin/env bash
# DIAGNOSTIC VERSION: prints what the checker says in every mode.
set -uo pipefail

VERSION=1.70.1
case "$(uname -s)" in
    Linux)
        asset=luau-lsp-linux-x86_64.zip
        sha=1a2ea1ae4f98f8946cefd970a4b54853e11ab4775e6932faa7f60cf920346567
        ;;
    Darwin)
        asset=luau-lsp-macos.zip
        sha=7d3936e8dec6dc77547abd061d2e950da392562a5f94858b880f23dba50d84cd
        ;;
    *)
        asset=luau-lsp-win64.zip
        sha=64225b9738102a30de6ee09fb28e5a3827edc402ecf2a6364af570312468159f
        ;;
esac

cd "$(dirname "$0")/../.."
tools="${RUNNER_TEMP:-/tmp}/luau-lsp-$VERSION"
mkdir -p "$tools"
curl -fsSL --retry 3 -o "$tools/$asset" \
    "https://github.com/JohnnyMorganz/luau-lsp/releases/download/$VERSION/$asset"
if command -v sha256sum >/dev/null; then
    got=$(sha256sum "$tools/$asset" | cut -d' ' -f1)
else
    got=$(shasum -a 256 "$tools/$asset" | cut -d' ' -f1)
fi
echo "sha $got (want $sha)"
unzip -oq "$tools/$asset" -d "$tools"
ls -la "$tools"
luau_lsp="$tools/luau-lsp"
"$luau_lsp" --version
"$luau_lsp" analyze --help

defs=crates/lore-script/types/lore.d.luau
fixtures=crates/lore-script/tests/types
printf '{ "languageMode": "strict" }\n' > "$tools/strict.luaurc"

run() {
    echo "=== $*"
    "$luau_lsp" analyze --platform=standard "--defs=@lore=$defs" "$@" 2>&1
    echo "--- exit $?"
}

for solver in false true; do
    for mode in nonstrict strict; do
        extra=()
        [ "$mode" = strict ] && extra=("--base-luaurc=$tools/strict.luaurc")
        echo "##### solver v2=$solver mode=$mode"
        run "--flag:LuauSolverV2=$solver" "${extra[@]}" crates/lore/template/main.luau examples
        run "--flag:LuauSolverV2=$solver" "${extra[@]}" "$fixtures/everything.luau"
        run "--flag:LuauSolverV2=$solver" "${extra[@]}" "$fixtures/mistakes.luau"
    done
done

sed 's/^declare class \(.*\)$/declare extern type \1 with/' "$defs" > "$tools/extern.d.luau"
echo "##### extern type form"
"$luau_lsp" analyze --platform=standard "--defs=@lore=$tools/extern.d.luau" "$fixtures/everything.luau" 2>&1
echo "--- exit $?"
echo "##### broken definitions"
echo "declare game: {" > "$tools/broken.d.luau"
"$luau_lsp" analyze --platform=standard "--defs=@lore=$tools/broken.d.luau" "$fixtures/everything.luau" 2>&1 | head -5
echo "--- exit $?"
echo "##### no definitions"
"$luau_lsp" analyze --platform=standard examples 2>&1 | head -5
exit 1
