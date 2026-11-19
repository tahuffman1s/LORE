#!/usr/bin/env bash
# Type-checks lore.d.luau, the template and the example games with the Luau
# language server, the type checker an author's editor runs.
#
# Run from anywhere: bash .github/scripts/check-types.sh
# It downloads one pinned luau-lsp release, checks the download against a
# known SHA-256, and runs `luau-lsp analyze`. Written for the bash on the
# Linux, Windows and macOS test machines, which on macOS is bash 3.2.
set -euo pipefail

# The release to check against. To move to a newer one, change the version
# and the three SHA-256 sums, which GitHub lists for each release file:
#   gh api repos/JohnnyMorganz/luau-lsp/releases/tags/<version> --jq '.assets[] | "\(.name) \(.digest)"'
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
    MINGW* | MSYS* | CYGWIN*)
        asset=luau-lsp-win64.zip
        sha=64225b9738102a30de6ee09fb28e5a3827edc402ecf2a6364af570312468159f
        ;;
    *)
        echo "check-types: no luau-lsp download is listed for $(uname -s). Add one to this script." >&2
        exit 1
        ;;
esac

cd "$(dirname "$0")/../.."
tools="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/luau-lsp-$VERSION"
mkdir -p "$tools"
curl -fsSL --retry 3 -o "$tools/$asset" \
    "https://github.com/JohnnyMorganz/luau-lsp/releases/download/$VERSION/$asset"
# Read from standard input, so no file name (and no Windows backslash) is in
# what comes back.
if command -v sha256sum >/dev/null; then
    got=$(sha256sum <"$tools/$asset" | cut -d' ' -f1)
else
    got=$(shasum -a 256 <"$tools/$asset" | cut -d' ' -f1)
fi
if [ "$got" != "$sha" ]; then
    echo "check-types: $asset is not the file this script expects (SHA-256 $got, expected $sha)." >&2
    echo "If luau-lsp $VERSION was published again, check the release and update the sum in this script." >&2
    exit 1
fi
unzip -oq "$tools/$asset" -d "$tools"
luau_lsp="$tools/luau-lsp"
echo "luau-lsp $("$luau_lsp" --version)"

defs=crates/lore-script/types/lore.d.luau
fixtures=crates/lore-script/tests/types
failed=0

# analyze <new solver: true|false> <files and folders...>
# Prints what the checker says and gives back its exit code: 0 for no mistakes.
analyze() {
    local new_solver=$1
    shift
    "$luau_lsp" analyze --platform=standard "--definitions=@lore=$defs" \
        "--flag:LuauSolverV2=$new_solver" "$@" 2>&1
}

# 1. The definitions load, and the scripts an author reads first show no
#    mistakes. The template and examples are checked twice: as an editor
#    shows them by default (no `--!strict` line, so Luau's default mode),
#    and as one set to strict shows them, where every value has to have a
#    known type. everything.luau is strict by its first line and uses every
#    function. Both of Luau's type solvers are asked: 1.70.1 uses the old
#    one unless told otherwise, later releases the new one.
for new_solver in false true; do
    for mode in default strict; do
        echo "Checking the template, the examples and everything.luau (new solver: $new_solver, $mode mode)"
        strict=()
        if [ "$mode" = strict ]; then
            strict=(--base-luaurc=.github/scripts/strict.luaurc)
        fi
        if ! out=$(analyze "$new_solver" ${strict[@]+"${strict[@]}"} crates/lore/template examples "$fixtures/everything.luau"); then
            echo "$out"
            case "$out" in
                *"Failed to read definitions file"*)
                    echo "check-types: lore.d.luau itself does not load in the Luau language server. Fix the SyntaxError lines above in $defs." >&2
                    ;;
                *)
                    echo "check-types: the type checker found mistakes. Fix the script, or fix $defs if it describes the engine wrongly." >&2
                    ;;
            esac
            failed=1
        fi
    done
done

# 2. The check can fail: every line marked `wrong` in mistakes.luau is
#    reported, and no other line. With the solver an editor uses by default.
echo "Checking that every mistake in mistakes.luau is caught"
out=$(analyze false "$fixtures/mistakes.luau" || true)
reported=$(echo "$out" | sed -n 's/^.*mistakes\.luau(\([0-9]*\),[0-9]*): .*$/\1/p' | sort -n -u | tr '\n' ' ')
expected=$(grep -n -e '-- wrong' "$fixtures/mistakes.luau" | cut -d: -f1 | sort -n -u | tr '\n' ' ')
if [ "$reported" != "$expected" ] || [ -z "$expected" ]; then
    echo "$out"
    echo "check-types: $fixtures/mistakes.luau has lines marked wrong at: $expected" >&2
    echo "but the type checker reported lines: $reported" >&2
    echo "A marked line that is not reported means $defs is too loose there; a reported line that is not marked needs a \`-- wrong\` comment or a fix." >&2
    failed=1
fi

if [ "$failed" = 0 ]; then
    echo "The Luau type checker agrees with lore.d.luau."
fi
exit "$failed"
