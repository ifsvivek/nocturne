//! Every message the root handles. Results of network work carry the page id / epoch they were
//! requested for and are dropped when the user has moved on.
use crate::account::{self, picker};
use crate::keys::Key;
use crate::task::Handoff;
use iced::widget::image::Handle;
use iced::{Size, window};
use nocturne_core::account::picker::DefaultChange;
use nocturne_core::catalog::Slot;
use nocturne_core::source::{ConnectError, Media, Source, SourceId};
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
    Sources,
}

/// What the Sources page can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrcAct {
    Close,
    AddServer,
    AddFolder,
    SignIn(SourceId),
    /// Make this server the one the account menu acts on.
    Use(SourceId),
    Rescan(SourceId),
    Folders(SourceId),
    Toggle(SourceId),
    Move(SourceId, i32),
    AskRemove(SourceId),
    ConfirmRemove,
    CancelRemove,
}

/// A local scan's report to the root.
#[derive(Clone, Debug)]
pub enum ScanMsg {
    Progress(nocturne_local::Progress),
    Done {
        cancelled: bool,
        error: Option<String>,
    },
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
    /// The saved sources and the catalog built from them (keyring and index reads, off the UI thread).
    Loaded(Handoff<super::catalog::Loaded>),
    /// One source's libraries for the Home page `pid`, answering request `epoch` (nav links).
    Fan(u64, u64, SourceId, Option<Vec<Media>>),
    /// Reachability of a server: its id and whether it answered.
    Pinged(SourceId, bool),
    /// A source slot rebuilt off the UI thread (enabled again, re-signed in).
    SlotReady(Handoff<Slot>),
    /// The stored login of a server, to make it the one the account menu acts on.
    SessionLoaded(SourceId, Option<(String, Session)>),
    /// Progress of the local scan `gen` of a source.
    Scan(SourceId, u64, ScanMsg),
    /// Sources page buttons.
    Src(SrcAct),
    /// Local folders screen.
    Folder(super::folders::Msg),
    CycleFilter,
    OpenSources,
    /// Debug builds only: a window capture for `NOCTURNE_SNAP`.
    Snapped(window::Screenshot, usize),
    /// Own profile read: (user id, picture tag).
    Profile(String, Option<Option<String>>),
    /// A profile picture fetched: (cache key, decoded handle).
    UserPic(String, Option<Handle>),
    /// The on-disk image cache finished opening (after the first frame).
    DiskReady(Option<std::sync::Arc<nocturne_core::imgcache::DiskCache>>),
    /// First-run welcome screen.
    Welcome(super::welcome::Msg),
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
