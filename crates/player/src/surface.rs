//! [`VideoSurface`]: the window the external mpv draws into when the picture plays inside the app
//! window (`--wid`). Windows only; elsewhere it is a stub that never exists, so the UI needs no
//! `cfg`. The surface is a child window of the app's HWND, created and moved on the UI thread.
//! The UI toolkit cannot draw over a child window, so the UI shrinks it for the controls band and
//! punches holes in it ([`VideoSurface::set_cutouts`], a window region) where a menu or panel is
//! drawn; the toolkit's pixels show through the holes. mpv's pointer input comes back over IPC
//! ([`crate::Event::Input`]).

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, DeleteObject, RGN_DIFF, SetWindowRgn,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GW_CHILD, GetWindow, SW_HIDE, SW_SHOWNA,
        SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, ShowWindow, WS_CHILD,
        WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    };

    /// `SS_BLACKRECT`: the predefined STATIC class paints it black, so there is no white flash
    /// before mpv's first frame, and no window procedure of ours is needed.
    const SS_BLACKRECT: u32 = 0x0004;

    type Rect = (i32, i32, i32, i32);

    pub struct VideoSurface {
        hwnd: HWND,
        parent: HWND,
        rect: Rect,
        /// Holes in parent-client device pixels `(x, y, w, h)`.
        cuts: Vec<Rect>,
        applied: Option<(Rect, Vec<Rect>)>,
        shown: bool,
    }

    impl VideoSurface {
        /// A child window of `parent` (an HWND), hidden (no `WS_VISIBLE`) until
        /// [`Self::set_bounds`] gives it a size. `None` if Windows refuses (the caller falls
        /// back to mpv's own window).
        pub fn create(parent: isize) -> Option<Self> {
            let parent = parent as HWND;
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: plain Win32 call on the UI thread with a valid parent handle.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    null(),
                    WS_CHILD | WS_CLIPSIBLINGS | WS_CLIPCHILDREN | SS_BLACKRECT,
                    0,
                    0,
                    1,
                    1,
                    parent,
                    null_mut(),
                    null_mut(),
                    null(),
                )
            };
            (!hwnd.is_null()).then_some(Self {
                hwnd,
                parent,
                rect: (0, 0, 0, 0),
                cuts: Vec::new(),
                applied: None,
                shown: false,
            })
        }

        /// The HWND to pass to mpv as `--wid`.
        pub fn hwnd(&self) -> isize {
            self.hwnd as isize
        }

        /// Places the window at a rectangle in the parent's client area, in logical pixels
        /// (`scale` converts to device pixels). A rectangle with no area hides it.
        pub fn set_bounds(&mut self, x: f32, y: f32, w: f32, h: f32, scale: f32) {
            let s = if scale > 0.0 { scale } else { 1.0 };
            let r = (
                (x * s).round() as i32,
                (y * s).round() as i32,
                (w * s).round() as i32,
                (h * s).round() as i32,
            );
            if r.2 < 2 || r.3 < 2 {
                self.set_visible(false);
                return;
            }
            if r != self.rect {
                self.rect = r;
                // SAFETY: own window on the UI thread. mpv's window inside lives in another
                // process: the async flag posts instead of waiting for it, so a stuck mpv cannot
                // freeze the UI.
                unsafe {
                    SetWindowPos(
                        self.hwnd,
                        null_mut(),
                        r.0,
                        r.1,
                        r.2,
                        r.3,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    let inner = GetWindow(self.hwnd, GW_CHILD);
                    if !inner.is_null() {
                        SetWindowPos(
                            inner,
                            null_mut(),
                            0,
                            0,
                            r.2,
                            r.3,
                            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                        );
                    }
                }
            }
            self.set_visible(true);
            self.apply_region();
        }

        /// Rectangles in the parent's client area (logical pixels) where the UI draws over the
        /// picture: the window is cut out there. Takes effect with the next bounds.
        pub fn set_cutouts(&mut self, cuts: &[(f32, f32, f32, f32)], scale: f32) {
            let s = if scale > 0.0 { scale } else { 1.0 };
            self.cuts = cuts
                .iter()
                .map(|c| {
                    (
                        (c.0 * s).floor() as i32,
                        (c.1 * s).floor() as i32,
                        (c.2 * s).ceil() as i32 + 1,
                        (c.3 * s).ceil() as i32 + 1,
                    )
                })
                .collect();
            self.apply_region();
        }

        /// The window region is the window's rectangle minus the cuts, in window coordinates.
        fn apply_region(&mut self) {
            if !self.shown {
                return;
            }
            let want = Some((self.rect, self.cuts.clone()));
            if self.applied == want {
                return;
            }
            self.applied = want;
            // SAFETY: GDI regions on the UI thread; after a successful SetWindowRgn the system
            // owns the region, so it is only deleted here when the call fails.
            unsafe {
                if self.cuts.is_empty() {
                    SetWindowRgn(self.hwnd, null_mut(), 1);
                    return;
                }
                let rgn = CreateRectRgn(0, 0, self.rect.2, self.rect.3);
                for c in &self.cuts {
                    let (x, y) = (c.0 - self.rect.0, c.1 - self.rect.1);
                    let one = CreateRectRgn(x, y, x + c.2, y + c.3);
                    CombineRgn(rgn, rgn, one, RGN_DIFF);
                    DeleteObject(one as _);
                }
                if SetWindowRgn(self.hwnd, rgn, 1) == 0 {
                    DeleteObject(rgn as _);
                }
            }
        }

        pub fn set_visible(&mut self, v: bool) {
            if v != self.shown {
                self.shown = v;
                // SAFETY: own window, UI thread.
                unsafe { ShowWindow(self.hwnd, if v { SW_SHOWNA } else { SW_HIDE }) };
            }
        }

        /// Keyboard focus back to the app window (mpv's window may have taken it on a click).
        pub fn focus_parent(&self) {
            // SAFETY: the app's own window, UI thread.
            unsafe { SetFocus(self.parent) };
        }
    }

    impl Drop for VideoSurface {
        fn drop(&mut self) {
            // SAFETY: own window, UI thread; destroys mpv's child with it.
            unsafe { DestroyWindow(self.hwnd) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct VideoSurface;

    impl VideoSurface {
        pub fn create(_: isize) -> Option<Self> {
            None
        }
        pub fn hwnd(&self) -> isize {
            0
        }
        pub fn set_bounds(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32) {}
        pub fn set_visible(&mut self, _: bool) {}
        pub fn set_cutouts(&mut self, _: &[(f32, f32, f32, f32)], _: f32) {}
        pub fn focus_parent(&self) {}
    }
}

pub use imp::VideoSurface;
