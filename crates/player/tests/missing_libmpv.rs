//! A missing libmpv must surface as an error from the probe and from `EmbeddedPlayer::new`, never a panic.
//! Own test binary: the library is resolved once per process, so the override must be set before first use.

#[test]
fn missing_libmpv_is_reported() {
    // SAFETY: single test in this binary, nothing else reads the environment concurrently.
    unsafe { std::env::set_var("NOCTURNE_LIBMPV", "/nonexistent/libmpv.so.2") };
    let err = nocturne_player::libmpv_available().unwrap_err();
    assert!(err.contains("cannot load"), "{err}");
    let r = nocturne_player::EmbeddedPlayer::new(std::ptr::null_mut(), std::ptr::null_mut());
    assert!(r.is_err());
    assert!(!nocturne_player::install_hint().is_empty());
}
