//! Windows-only: embed the app icon and VS_VERSION_INFO into keyroost.exe.
//!
//! Token2's signed v0.7.5 exe carried an icon THEY injected during signing
//! (an added .rsrc section), which made signed builds differ from CI's by
//! more than a signature. Owning the icon here puts it in every keyroost.exe
//! (CI, `cargo install`, and signed builds become our-exact-bytes plus a
//! signature). The `winresource` build-dep is gated to Windows hosts in
//! Cargo.toml, and this whole file is a no-op elsewhere, so the Linux/macOS
//! builds and the workspace MSRV story are untouched.

#[cfg(windows)]
fn main() {
    // The icon lives INSIDE this crate on purpose: cargo only packages files
    // under the package root, so a path like `../../packaging/...` would be
    // missing from the published .crate and `cargo install keyroost` would
    // fail at the panic below. Regenerate it with packaging/icons/gen-ico.py
    // after editing the hicolor PNGs — that script writes straight to here,
    // so this stays the single copy.
    println!("cargo:rerun-if-changed=assets/keyroost.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/keyroost.ico");
    // FileVersion/ProductVersion default to CARGO_PKG_VERSION; fill in the
    // human-facing strings Explorer shows in Properties → Details.
    res.set("ProductName", "keyroost_l10n");
    res.set(
        "FileDescription",
        "KEYROOST_L10N",
    );
    res.set("OriginalFilename", "keyroost.exe");
    res.set("FileVersion", "1.0.15");
    res.set("ProductVersion", "1.0.15");
    if let Err(e) = res.compile() {
        // Fail the build rather than silently shipping an icon-less exe —
        // that is the exact regression this build script exists to prevent.
        panic!("embedding Windows resources failed: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
