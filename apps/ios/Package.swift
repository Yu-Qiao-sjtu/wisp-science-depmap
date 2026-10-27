// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "WispRemote",
    platforms: [.iOS(.v17), .macOS(.v13)],
    products: [.library(name: "WispRemote", targets: ["WispRemote"])],
    targets: [
        .target(name: "WispRemote"),
        .testTarget(name: "WispRemoteTests", dependencies: ["WispRemote"]),
    ]
)
