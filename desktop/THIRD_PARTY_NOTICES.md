# Desktop third-party notices

The desktop wrapper is licensed under Apache-2.0, Copyright 2026 Khun.
The upstream Bifrost LICENSE and THIRD_PARTY_NOTICES.md are included unchanged.

Tauri 2 and its single-instance, autostart, opener, and clipboard plugins are
used under Apache-2.0 (dual MIT/Apache-2.0). Rust windows-sys, getrandom,
serde, serde_json, zeroize and reqwest are also used under Apache-2.0.
Exact versions, sources and integrity checksums are in src-tauri/Cargo.lock.
Credential storage uses the Windows Credential Manager API directly; no
third-party secret storage service is involved.

The optional `with-webview2` portable package includes Microsoft's unmodified WebView2 Fixed Version
Runtime. This component is proprietary Microsoft software, distributed under
the Microsoft Edge WebView2 Runtime redistribution terms, not Apache-2.0.
Its license and third-party notices are preserved in the WebView2 directory.
See https://developer.microsoft.com/microsoft-edge/webview2/ and
https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution.

The independent geometric bif-app icon is original artwork by Khun.
It is not the Bifrost or Maxim logo.
