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

/// Which of the engine's scenes an event belongs to: the whole Home page (header included), the header alone on other pages, the library page's body, or the search overlay.
#[cfg(feature = "theme-engine")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Home,
    Header,
    Page,
    Search,
}

/// Which fan-out on the Home page an answer belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FanKind {
    Libs,
    Resume,
    NextUp,
}

/// A cell of the page in view (Home strip position, grid index, search hit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    Hero,
    /// Layout row, column.
    Home(usize, usize),
    Grid(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellAct {
    Open,
    Play,
    Hover,
    Fav,
    Played,
}

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
    Downloads,
    Admin,
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
    /// Edit the server's extra HTTP header.
    Header(SourceId),
    HeaderSave,
    HeaderClear,
    HeaderCancel,
    /// The next choice of whose UI this source (or "All", `None`) is browsed with.
    Layout(Option<SourceId>),
    /// The home-sections editor of this server, and its buttons.
    Sections(SourceId),
    SecToggle(usize),
    SecShift(usize, i32),
    SecSave,
    SecCancel,
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
    /// An event of the theme engine's Home or header widget.
    #[cfg(feature = "theme-engine")]
    Eng(Which, crate::widget::styled::StyledEvent),
    /// The Theme Studio's panel and pick mode.
    #[cfg(feature = "theme-engine")]
    Studio(super::studio::Msg),
    /// The hero carousel of the engine's Home: previous (-1) or next (1) slide, a dot, the description's open state, and the facts the timer pauses on (window focus, a pressed pointer).
    #[cfg(feature = "theme-engine")]
    HeroStep(isize),
    #[cfg(feature = "theme-engine")]
    HeroGoto(usize),
    #[cfg(feature = "theme-engine")]
    HeroToggle,
    #[cfg(feature = "theme-engine")]
    WinFocus(bool),
    #[cfg(feature = "theme-engine")]
    Pointer(bool),
    CloseRequested(window::Id),
    Back,
    Forward,
    /// Developer-token connect finished (`bool`: the token has admin rights).
    Connected(Result<Source, ConnectError>),
    AdminProbed(bool),
    Retry,
    RetryNow,
    /// The daily update check finished (`None`: not due, off, dev build or failed).
    Update(Option<nocturne_core::update::UpdateState>),
    UpdateCopy(&'static str),
    UpdateOpen(String),
    UpdateDismiss,
    /// Banner "Install": download, verify, swap.
    UpdateInstall(nocturne_core::update::Artifact),
    /// Download finished (verified file, or why not); the swap follows on the same task.
    UpdateInstalled(Result<std::path::PathBuf, String>),
    /// Banner "Restart to update".
    UpdateRestart,
    /// Whether PackageKit can be asked to update a system-package install (asked once, when the banner first applies).
    UpdatePkAvailable(bool),
    /// Banner "Update now": the version to update to.
    UpdatePk(String),
    UpdatePkDone(Result<(), String>),
    Account(account::Msg),
    PickerAct(picker::Act),
    Switched {
        acct: nocturne_session::Account,
        via_picker: bool,
        default: DefaultChange,
        r: Result<Session, SwitchFail>,
    },
    /// Typed into the header dialog: (is the value field, text).
    HeaderInput(bool, String),
    /// The saved header of a server, read from the keyring for the dialog.
    HeaderLoaded(SourceId, Option<String>),
    /// A header save / removal finished.
    HeaderDone(SourceId, Result<(), String>),
    /// The saved sources and the catalog built from them (keyring and index reads, off the UI thread).
    Loaded(Handoff<super::catalog::Loaded>),
    /// One source's answer for Home page `pid` (libraries, continue watching, next up), to request `epoch`.
    Fan(u64, FanKind, u64, SourceId, Option<Vec<Media>>),
    /// A server's own layout and branding (Server UI); `None` when the read failed.
    ServerUi(SourceId, Option<nocturne_core::server_ui::ServerUi>),
    /// The icon font of a server's web client reached the text shaper.
    #[cfg(feature = "theme-engine")]
    JfFont(SourceId),
    /// The home-sections editor's save finished.
    SectionsSaved(SourceId, bool),
    /// One library's latest items on Home page `pid`: (pid, request epoch, source, library id, items).
    Latest(u64, u64, SourceId, String, Option<Vec<Media>>),
    /// Provider ids of the series behind some episodes (for matching episodes across sources).
    SeriesIds(SourceId, Vec<(String, nocturne_core::source::ProviderIds)>),
    /// A decoded image for cache `key` (`bool`: the hero cache).
    Image(String, bool, Option<Handle>),
    Scale(f32),
    /// Re-read the window scale (after Couch mode changed the app scale).
    Rescale,
    /// The header link strip scrolled: offset, viewport width, content width.
    NavScrolled(f32, f32, f32),
    /// A chevron on the link strip: page it left (-1) or right (+1).
    NavPage(f32),
    /// Gamepad: connected count / a button press.
    Pad(crate::pad::Ev),
    /// Logical size of the monitor under the window (Automatic couch mode).
    Monitor(Option<Size>),
    /// Something done to a cell of the page in view.
    Cell(At, CellAct),
    /// Play this item (a series resolves to its next episode first).
    PlayItem(Media),
    /// Start playback of an item with the queue that follows it. THE hook for the player page.
    Play(Media, Vec<Media>),
    PlayTarget(
        Media,
        Vec<Media>,
        Option<String>,
        Result<nocturne_core::backend::PlayTarget, nocturne_core::backend::PlayError>,
    ),
    Player(crate::player::Msg),
    Admin(crate::admin::Msg),
    SyncPlay(crate::syncplay::Msg),
    Queue(crate::queue::Msg),
    Devices(crate::account::devices::Msg),
    /// A SyncPlay group wants its item played: (source, list, index, start seconds, target).
    GroupPlay(
        SourceId,
        Vec<Media>,
        usize,
        f64,
        Result<nocturne_core::backend::PlayTarget, nocturne_core::backend::PlayError>,
    ),
    /// Remote control: a `Play` from another app resolved to items (list, index, start seconds, queue only).
    RemoteItems(Vec<Media>, usize, f64, bool),
    RemoteTarget(
        Vec<Media>,
        usize,
        f64,
        Result<nocturne_core::backend::PlayTarget, nocturne_core::backend::PlayError>,
    ),
    /// A message of detail page `pid`.
    Detail(u64, crate::detail::Msg),
    /// A write the detail page asked for finished.
    DetailWrote(u64, crate::detail::Write, Result<(), &'static str>),
    HomeScroll(f32, f32),
    HomeStrip(usize, f32),
    /// A row's arrow was pressed: strip index and direction (-1 back, 1 forward).
    HomeStripPage(usize, f32),
    Browse(u64, super::browse::BMsg),
    Search(super::search::SMsg),
    /// The stored login of a server, to build its write hooks (played / favourite).
    HooksLoaded(SourceId, Option<(String, Session)>),
    /// A server's public name (`/System/Info/Public`).
    ServerName(SourceId, Option<String>),
    ToggleLibMenu,
    RefreshResume,
    Toast(String),
    /// A local played-state write finished: (source, item, new value, ok).
    LocalWrite(SourceId, String, bool, bool),
    SearchScrolled(f32),
    OpenKind(nocturne_core::unified::UKind),
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
    Online(super::online::Msg),
    Trakt(super::trakt::Msg),
    Smart(super::smart::Msg),
    Cascade(super::cascade::Msg),
    Downloads(super::downloads::Msg),
    CycleFilter,
    OpenSources,
    Themes(super::themes::Msg),
    /// Debug builds only: a window capture for `NOCTURNE_SNAP`.
    Snapped(window::Screenshot, usize),
    /// Own profile read: (user id, picture tag).
    Profile(String, Option<Option<String>>),
    /// A profile picture fetched: (cache key, decoded handle).
    UserPic(String, Option<Handle>),
    /// The on-disk image cache finished opening (after the first frame).
    DiskReady(Option<std::sync::Arc<nocturne_core::imgcache::DiskCache>>),
    /// The first-run walkthrough.
    Onboarding(crate::onboarding::Msg),
    /// Settings > Run setup again.
    RunSetup,
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
