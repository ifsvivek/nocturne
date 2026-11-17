//! Non-Linux stand-in for the in-window player: it can never be constructed, so the app always
//! takes the external-mpv path there (docs/PLATFORMS.md: native embedding on macOS/Windows is
//! later work). The method set mirrors `embedded.rs` so callers compile unchanged.
use crate::{Buffering, Error, Event, Result, State};
use std::ffi::c_void;
use std::sync::mpsc::Receiver;

pub enum EmbeddedPlayer {}

/// No Wayland, so no compositor colour management either.
pub fn colour_management_available(_display: *mut c_void) -> bool {
    false
}

impl EmbeddedPlayer {
    pub fn new(display: *mut c_void, parent: *mut c_void) -> Result<Self> {
        Self::with_buffering(display, parent, Buffering::default())
    }

    pub fn with_buffering(_: *mut c_void, _: *mut c_void, _: Buffering) -> Result<Self> {
        Err(Error::Mpv(
            "in-window video is not available on this OS yet".into(),
        ))
    }

    pub fn set_bounds(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32) {
        match *self {}
    }
    pub fn events(&self) -> &Receiver<Event> {
        match *self {}
    }
    pub fn load(&self, _: &str, _: &[&str]) -> Result<()> {
        match *self {}
    }
    pub fn play(&self) -> Result<()> {
        match *self {}
    }
    pub fn pause(&self) -> Result<()> {
        match *self {}
    }
    pub fn toggle(&self) -> Result<()> {
        match *self {}
    }
    pub fn seek_to(&self, _: f64) -> Result<()> {
        match *self {}
    }
    pub fn seek_by(&self, _: f64) -> Result<()> {
        match *self {}
    }
    pub fn set_volume(&self, _: f64) -> Result<()> {
        match *self {}
    }
    pub fn stop(&self) -> Result<()> {
        match *self {}
    }
    pub fn command(&self, _: &[&str]) -> Result<()> {
        match *self {}
    }
    pub fn prop(&self, _: &str) -> Option<serde_json::Value> {
        match *self {}
    }
    pub fn state(&self) -> State {
        match *self {}
    }
}
