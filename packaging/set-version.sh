#!/usr/bin/env bash
# Sets the app's version, for example: packaging/set-version.sh 1.2.0
#
# The version lives in Cargo.toml and everything else follows from it: `gradebook --version`,
# the Windows file properties, the .deb, the AppImage, the macOS bundle and the name of every
# installer. The release workflow runs this before it builds, so a release gets the version
# typed into "Run workflow" without a commit for it. Run locally it changes Cargo.toml and
# Cargo.lock and nothing else; `git checkout Cargo.toml Cargo.lock` puts them back.
set -euo pipefail
cd "$(dirname "$0")/.."

version=${1:?usage: packaging/set-version.sh MAJOR.MINOR.PATCH, for example 1.2.0}
pattern='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
if ! [[ $version =~ $pattern ]]; then
    echo "set-version: '$version' is not a version like 1.2.0" >&2
    exit 1
fi

# Replaces the quoted version on one line of FILE, using the awk program in $2. Everything
# else, including the file's line endings and permissions, stays as it was. The program
# must exit with an error if it never found the line.
rewrite() {
    local file=$1 program=$2 tmp
    tmp=$(mktemp "${TMPDIR:-/tmp}/set-version.XXXXXX")
    if ! awk -v version="$version" "$program" "$file" > "$tmp"; then
        rm -f "$tmp"
        echo "set-version: couldn't find the version in $file" >&2
        exit 1
    fi
    cat "$tmp" > "$file"
    rm -f "$tmp"
}

# Cargo.toml: the package's own `version = "..."`, which is the first such line.
rewrite Cargo.toml '
    !done && /^version = "/ { sub(/"[^"]*"/, "\"" version "\""); done = 1 }
    { print }
    END { if (!done) exit 1 }
'

# Cargo.lock: the entry for this package, so that `cargo build --locked` keeps working.
if [ -f Cargo.lock ]; then
    rewrite Cargo.lock '
        /^name = "gradebook"/ { hit = 1 }
        hit && /^version = "/ { sub(/"[^"]*"/, "\"" version "\""); hit = 0; done = 1 }
        { print }
        END { if (!done) exit 1 }
    '
fi

echo "The version is now $version"
