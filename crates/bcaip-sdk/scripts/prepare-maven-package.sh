#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <native-lib> [jna-resource-prefix]" >&2
  exit 1
fi

native_lib="$1"
resource_prefix="${2:-$(crates/bcaip-sdk/scripts/maven-resource-prefix.sh)}"

case "$resource_prefix" in
darwin-aarch64 | darwin-x86-64 | linux-x86-64 | linux-aarch64 | win32-x86-64) ;;
*)
  echo "unsupported JNA resource prefix: $resource_prefix" >&2
  exit 1
  ;;
esac

if [ ! -f "$native_lib" ]; then
  echo "native library not found: $native_lib" >&2
  exit 1
fi

bindgen="target/release/bcaip-uniffi-bindgen"

if [ ! -x "$bindgen" ]; then
  cargo build -p bcaip-sdk --features uniffi --release -q
fi

maven_dir="crates/bcaip-sdk/maven"
kotlin_dir="$maven_dir/src/main/kotlin"
support_kotlin_dir="$maven_dir/src/support/kotlin"
resources_dir="$maven_dir/src/main/resources"
generated_package_dir="$kotlin_dir/io/github/bezotcorp/bcaip"

rm -rf \
  "${generated_package_dir:?}" \
  "${resources_dir:?}/${resource_prefix:?}"

mkdir -p \
  "${kotlin_dir:?}" \
  "${resources_dir:?}/${resource_prefix:?}" \
  "${resources_dir:?}/META-INF"

cp LICENSE "$resources_dir/META-INF/LICENSE"

"$bindgen" generate \
  --library "$native_lib" \
  --config crates/bcaip-sdk/uniffi.toml \
  --language kotlin \
  --no-format \
  --out-dir "$kotlin_dir" \
  2>/dev/null

python3 - "$generated_package_dir/bcaip.kt" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
text = path.read_text()

needle = 'private fun findLibraryName(componentName: String): String {\n'
replacement = needle + '    NativeLibraryLoader.ensureLoaded()\n'

if needle not in text:
    raise SystemExit(
        f"could not find findLibraryName in generated Kotlin bindings: {path}"
    )

path.write_text(text.replace(needle, replacement, 1))
PY

cp -R "$support_kotlin_dir"/. "$kotlin_dir"/
cp "$native_lib" "$resources_dir/$resource_prefix/"
