#!/usr/bin/env bash
# Generates the core for PWMKit (D41 §2) into apps/apple/PWMKit/Generated/
# (gitignored): the UniFFI Swift file, and the native library it calls.
#
#   apps/apple/PWMKit/generate.sh apple   # on a Mac with Xcode (`just apple`): an XCFramework
#                                         # for iOS, the iOS simulator and macOS (arm64 + x86_64)
#   apps/apple/PWMKit/generate.sh linux   # CI: a module map over target/debug/libpwm_ffi.so
set -euo pipefail

mode=${1:?usage: generate.sh apple|linux}
root=$(cd "$(dirname "$0")/../../.." && pwd)
out="$root/apps/apple/PWMKit/Generated"
work="$root/target/pwmkit"
cd "$root"

rm -rf "$out" "$work"
mkdir -p "$out/PWMCore" "$work/headers"

bindgen() {
    cargo run --locked -q -p pwm-ffi-bindgen -- generate --library "$1" \
        --language swift --out-dir "$work/swift" --no-format
    cp "$work/swift/pwm_ffi.swift" "$out/PWMCore/pwm_ffi.swift"
    cp "$work/swift/pwm_ffiFFI.h" "$work/headers/pwm_ffiFFI.h"
}

case "$mode" in
linux)
    cargo build --locked -p pwm-ffi
    bindgen target/debug/libpwm_ffi.so
    mkdir -p "$out/linux"
    cp "$work/headers/pwm_ffiFFI.h" "$out/linux/"
    cat > "$out/linux/module.modulemap" <<'EOF'
module pwm_ffiFFI {
    header "pwm_ffiFFI.h"
    link "pwm_ffi"
    export *
}
EOF
    ;;
apple)
    targets=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin x86_64-apple-darwin)
    rustup target add "${targets[@]}"
    # The package's minimums (Package.swift), so C code such as the bundled
    # SQLite is not built for the runner's newer OS.
    export MACOSX_DEPLOYMENT_TARGET=14.0 IPHONEOS_DEPLOYMENT_TARGET=17.0
    for target in "${targets[@]}"; do
        # Static libraries keep the symbols the app links against; drop debug info only.
        CARGO_PROFILE_RELEASE_STRIP=debuginfo cargo build --locked --release -p pwm-ffi --target "$target"
    done
    bindgen target/aarch64-apple-darwin/release/libpwm_ffi.dylib
    cat > "$work/headers/module.modulemap" <<'EOF'
module pwm_ffiFFI {
    header "pwm_ffiFFI.h"
    export *
}
EOF
    lipo -create target/aarch64-apple-darwin/release/libpwm_ffi.a target/x86_64-apple-darwin/release/libpwm_ffi.a \
        -output "$work/libpwm_ffi-macos.a"
    xcodebuild -create-xcframework \
        -library target/aarch64-apple-ios/release/libpwm_ffi.a -headers "$work/headers" \
        -library target/aarch64-apple-ios-sim/release/libpwm_ffi.a -headers "$work/headers" \
        -library "$work/libpwm_ffi-macos.a" -headers "$work/headers" \
        -output "$out/pwm_ffiFFI.xcframework"
    ;;
*)
    echo "usage: generate.sh apple|linux" >&2
    exit 2
    ;;
esac
echo "PWMKit: core generated into apps/apple/PWMKit/Generated ($mode)"
