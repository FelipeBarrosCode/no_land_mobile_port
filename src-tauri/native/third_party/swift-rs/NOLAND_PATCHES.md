# Noland compatibility patch

Source: `swift-rs` 1.0.8 from crates.io.

Xcode 27 internalizes non-public `@_cdecl` functions in optimized SwiftPM static
products. Upstream 1.0.8 promotes each package's own C exports with
`llvm-objcopy`, but `SwiftRs.o` is a dependency member inside `libTauri.a`, not a
separately linked product. The local patch also promotes that member only while
processing the Tauri archive. Copies embedded in plugin archives remain local,
preventing duplicate global symbols.
