// swift-tools-version:6.0
// The Swift package shared by the iOS app, its AutoFill extension and the
// macOS app (D41). `just apple` (on a Mac) generates the core's Swift file
// and XCFramework into Generated/ first; on Linux, CI generates the Swift
// file and links the native library built by cargo.

import PackageDescription

#if os(Linux)
// The native library from `cargo build -p pwm-ffi`; pass its folder with
// `swift test -Xlinker -L<dir>` and set LD_LIBRARY_PATH to run.
let core: Target = .systemLibrary(name: "pwm_ffiFFI", path: "Generated/linux")
#else
let core: Target = .binaryTarget(name: "pwm_ffiFFI", path: "Generated/pwm_ffiFFI.xcframework")
#endif

let package = Package(
    name: "PWMKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "PWMKit", targets: ["PWMKit"]),
        .library(name: "PWMDesign", targets: ["PWMDesign"]),
        .library(name: "PWMUI", targets: ["PWMUI"]),
    ],
    targets: [
        core,
        // The UniFFI Swift file, generated and never edited.
        .target(
            name: "PWMCore",
            dependencies: ["pwm_ffiFFI"],
            path: "Generated/PWMCore",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(name: "PWMKit", dependencies: ["PWMCore"]),
        .target(name: "PWMDesign"),
        // The screens (D42 §3): view models with no SwiftUI, tested on Linux
        // too, and SwiftUI views the iOS and macOS apps share.
        .target(name: "PWMUI", dependencies: ["PWMKit", "PWMDesign"]),
        .testTarget(name: "PWMKitTests", dependencies: ["PWMKit"]),
        .testTarget(name: "PWMDesignTests", dependencies: ["PWMDesign"]),
        .testTarget(name: "PWMUITests", dependencies: ["PWMUI"]),
    ]
)
