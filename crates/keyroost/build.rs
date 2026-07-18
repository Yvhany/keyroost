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
    // Re-run when the icon changes (regenerate it with
    // packaging/icons/gen-ico.py after editing the hicolor PNGs).
    println!("cargo:rerun-if-changed=../../packaging/icons/keyroost.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/icons/keyroost.ico");
    // FileVersion/ProductVersion default to CARGO_PKG_VERSION; fill in the
    // human-facing strings Explorer shows in Properties → Details.
    res.set("ProductName", "keyroost");
    res.set(
        "FileDescription",
        "keyroost — security key and TOTP token manager",
    );
    res.set("OriginalFilename", "keyroost.exe");
    if let Err(e) = res.compile() {
        // Fail the build rather than silently shipping an icon-less exe —
        // that is the exact regression this build script exists to prevent.
        panic!("embedding Windows resources failed: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
