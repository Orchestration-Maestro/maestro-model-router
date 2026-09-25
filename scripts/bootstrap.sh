#!/usr/bin/env bash
# One command to get from a fresh clone to a machine that can run the gate.
#
# mise installs every pinned tool listed in mise.toml, Just included, and refuses
# any download whose checksum differs from mise.lock. Something has to obtain
# mise before that can run. On Linux, fetch its checksum-verified release, then
# hand over to `just setup`.
#
# It does not install rustup. Provision that through your approved platform
# channel; no `curl | sh` bootstrap is included here.
set -euo pipefail

readonly MISE_VERSION=v2026.9.9
readonly MISE_SHA256=e4767e4854af5daeff2191b2bbdc94f834742a23efad591dbd33187861d41604

fail() {
  echo "$1" >&2
  exit 1
}

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

[[ "$(uname -s)/$(uname -m)" == Linux/x86_64 ]] ||
  fail 'Pinned tooling requires Linux x64.'
command -v rustup >/dev/null ||
  fail 'rustup is required. Install it through your approved platform channel, then run this again.'
command -v curl >/dev/null || fail 'curl is required.'
# The root rust-toolchain.toml is the one copy of the local toolchain version;
# the justfile reads the same line, so a bump happens in one file.
TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
[[ "$TOOLCHAIN" =~ ^1\.[0-9]+\.[0-9]+$ ]] ||
  fail 'rust-toolchain.toml must pin an exact stable version.'
readonly TOOLCHAIN

if [[ ! -x .tools/bin/mise || "$(.tools/bin/mise --version)" != "${MISE_VERSION#v} "* ]]; then
  echo "Fetching mise ${MISE_VERSION}"
  mkdir -p .tools/bin
  archive=$(mktemp)
  trap 'rm -f "$archive"' EXIT
  curl --fail --silent --show-error --location --output "$archive" \
    "https://github.com/jdx/mise/releases/download/${MISE_VERSION}/\
mise-${MISE_VERSION}-linux-x64.tar.gz"
  # An unverified download is how a supply chain is entered, so the digest is
  # checked before the archive is opened rather than after.
  echo "${MISE_SHA256}  ${archive}" | sha256sum --check --strict
  tar -xzf "$archive" -C .tools/bin --strip-components=2 mise/bin/mise
fi

export PATH="$PWD/.tools/bin:$PATH"

echo "Installing Rust ${TOOLCHAIN}"
rustup toolchain install "$TOOLCHAIN" --profile minimal \
  --component rustfmt,clippy,llvm-tools-preview --no-self-update

echo 'Installing pinned tools'
mise trust mise.toml
# --locked installs exactly the URLs mise.lock records and never rewrites it.
mise install --locked
# Just is one of the tools mise just installed. Run it from there once, so it
# can link the toolbelt into .tools/bin and wire the commit hook.
mise exec -- just setup

cat <<EOF

Ready. Add the tools to your PATH for this shell:

  export PATH="\$PWD/.tools/bin:\$PATH"

Then run the gate:

  just check
EOF
