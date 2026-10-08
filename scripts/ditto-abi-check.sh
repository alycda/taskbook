#!/usr/bin/env bash
# Print the C declarations that crates/taskbook-client/src/storage/ditto/ffi.rs
# depends on, as published in a given Ditto release, so an SDK bump can be
# diffed against the current bindings:
#
#   scripts/ditto-abi-check.sh 4.14.7 > /tmp/abi-4.14.7.txt
#   scripts/ditto-abi-check.sh 4.15.0 > /tmp/abi-4.15.0.txt
#   diff /tmp/abi-4.14.7.txt /tmp/abi-4.15.0.txt
#
# The declarations come from the C header that Ditto embeds in its C++ SDK
# (`Ditto.h`, include guard `__RUST_DITTOFFI__`); the library we link
# (`libdittoffi.a`, the Rust-SDK artifact) exports the same ABI.
set -euo pipefail

version="${1:-}"
if [[ -z "$version" ]]; then
    echo "usage: $0 <ditto-version> [cpp-platform]" >&2
    exit 2
fi
platform="${2:-cpp-linux-x86_64}"
ffi_rs="$(dirname "$0")/../crates/taskbook-client/src/storage/ditto/ffi.rs"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

url="https://software.ditto.live/${platform}/Ditto/${version}/dist/Ditto.tar.gz"
echo "# source: $url" 
curl --fail --silent --show-error --location --output "$work/Ditto.tar.gz" "$url"
tar -xzf "$work/Ditto.tar.gz" -C "$work" Ditto.h

# Extract the embedded C header.
start="$(grep -n '^#ifndef __RUST_DITTOFFI__' "$work/Ditto.h" | head -1 | cut -d: -f1)"
end="$(awk -v s="$start" 'NR>s && /^#endif.*__RUST_DITTOFFI__/ {print NR; exit}' "$work/Ditto.h")"
sed -n "${start},${end}p" "$work/Ditto.h" > "$work/dittoffi.h"

# Every function our bindings declare.
names="$(grep -oE 'pub fn (dittoffi|ditto)_[a-z0-9_]+' "$ffi_rs" | awk '{print $3}' | sort -u)"

python3 - "$work/dittoffi.h" $names <<'PY'
import re, sys
src = open(sys.argv[1], encoding="utf-8", errors="replace").read()
for name in sys.argv[2:]:
    marker = "/* fn */ " + name + " ("
    i = src.find(marker)
    if i < 0:
        marker = "/* fn */ " + name + "("
        i = src.find(marker)
    if i < 0:
        print(f"{name}: NOT FOUND")
        continue
    pre = src[:i]
    j = max(pre.rfind(";"), pre.rfind("*/"), pre.rfind("}"))
    ret = " ".join(pre[j + 2:].split())
    k = src.find(");", i)
    params = " ".join(src[i + len("/* fn */ "):k].split())
    print(f"{ret} {params});")
PY

echo
echo "# struct layouts referenced above"
for t in slice_ref_uint8 slice_boxed_uint8 dittoffi_result_void dittoffi_result_CDitto_ptr \
         dittoffi_result_dittoffi_query_result_ptr dittoffi_result_dittoffi_sync_subscription_ptr \
         dittoffi_result_dittoffi_store_observer_ptr ArcDynFn0_void \
         BoxDynFnMut3_void_dittoffi_query_result_ptr_ArcDynFn0_void TransportConfigMode CLogLevel; do
    python3 - "$work/dittoffi.h" "$t" <<'PY'
import re, sys
src = re.sub(r"/\*.*?\*/", "", open(sys.argv[1], encoding="utf-8", errors="replace").read(), flags=re.S)
t = sys.argv[2]
m = re.search(r"typedef (struct|enum) " + re.escape(t) + r"\b\s*\{(.*?)\}\s*(\w+);", src, flags=re.S)
print(f"{t}: " + (" ".join(m.group(2).split()) if m else "NOT FOUND"))
PY
done
