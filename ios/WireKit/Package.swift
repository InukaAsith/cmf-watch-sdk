// swift-tools-version: 6.0
// Wire formats, sync and install of the workout iPhone app, without UIKit: tested on the Mac with `swift test`.
import PackageDescription

let package = Package(
    name: "WireKit",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [.library(name: "WireKit", targets: ["WireKit"])],
    targets: [
        .target(name: "WireKit"),
        .testTarget(name: "WireKitTests", dependencies: ["WireKit"], resources: [.copy("vectors.json")]),
    ]
)
