//! Every message the root handles. Results of network work carry the page id / epoch they were
//! requested for and are dropped when the user has moved on.
use crate::account::{self, picker};
use crate::keys::Key;
use iced::widget::image::Handle;
use iced::{Size, window};
use nocturne_core::account::picker::DefaultChange;
use nocturne_core::source::{ConnectError, Media, Source};
use nocturne_jellyfin_user::Session;
use std::time::Instant;

/// Account menu entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcctAct {
    Settings,
    Password,
    Authorize,
    Devices,
    Together,
    Switch,
    SignOut,
    /// Switch to remembered account `i` (index into `App::other_accounts`).
    SwitchTo(usize),
    /// Back to the "Who's watching?" screen (stays signed in until a profile is chosen).
    SwitchProfile,
    AddAccount,
    PlayQueue,
    ClearQueue,
    ProfileImage,
    RemoveImage,
    Language,
}

/// Why switching to a remembered account failed.
#[derive(Clone, Copy, Debug)]
pub enum SwitchFail {
    /// No token, or the server no longer accepts it: sign in again.
    Gone,
    Offline,
}

#[derive(Clone)]
pub enum Message {
    Key(Key),
    Tick(Instant),
    Frame(Instant),
    Resized(Size),
    CloseRequested(window::Id),
    Back,
    Forward,
    /// Developer-token connect finished (`bool`: the token has admin rights).
    Connected(Result<Source, ConnectError>),
    AdminProbed(bool),
    Retry,
    RetryNow,
    Account(account::Msg),
    PickerAct(picker::Act),
    Switched {
        acct: nocturne_session::Account,
        via_picker: bool,
        default: DefaultChange,
        r: Result<Session, SwitchFail>,
    },
    /// Libraries for the Home page `pid` (nav links).
    Libraries(u64, Option<Vec<Media>>),
    /// Own profile read: (user id, picture tag).
    Profile(String, Option<Option<String>>),
    /// A profile picture fetched: (cache key, decoded handle).
    UserPic(String, Option<Handle>),
    Pinged,
    // header
    GoHome,
    OpenLibrary(usize),
    OpenGlobal(usize),
    OpenAdmin,
    OpenSearch,
    ToggleMenu,
    CloseMenu,
    MenuAct(AcctAct),
    Crumb(usize),
    // settings
    SetRow(usize),
    SetChoose(usize, usize),
    SetClose,
    SetScrolled(f32, f32),
    CloseOverlays,
    PageRetry,
    Noop,
}
