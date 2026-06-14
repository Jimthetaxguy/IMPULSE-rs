# Native Island Contract

Native islands are macOS-specific affordances behind the Dioxus shell. They are
not a second UI architecture.

## Boundary

- Dioxus sends a `NativeIslandRequest` through the desktop host command surface.
- Rust routes the request to a `NativeIslandHost`.
- The native bridge returns a `NativeIslandResult`.
- Dioxus updates UI from that result and daemon snapshots.

Native code must not retain session, memory, terminal, or artifact state. It can
retain only the minimum lifecycle objects required by AppKit or Swift.

## Swift-Compatible Shape

Swift-only affordances should expose Objective-C-compatible classes:

```swift
import Foundation

@objc(ImpulseNativeIsland)
final class ImpulseNativeIsland: NSObject {
    @objc func handle(_ requestJson: String) -> String {
        // Decode NativeIslandRequest, perform the native affordance, and return
        // NativeIslandResult JSON. Do not store Impulse session state here.
        return "{}"
    }
}
```

Rust calls the bridge through `objc2` or another narrow host adapter.
Objective-C compatibility is the ABI boundary; Swift/AppKit can implement the
native behavior behind it. Legacy Tauri plugins are compatibility-only and must
not become the primary host architecture.
