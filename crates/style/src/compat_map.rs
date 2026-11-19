//! The Jellyfin web to NOCTURNE vocabulary tables used by `compat.rs` (see `docs/dev/THEME_COMPAT.md`). Jellyfin web 12 names on the left (cardBuilder, the header, the item page, the OSD), the NOCTURNE element or chain that plays the same role on the right.
use To::{Never as N, Same as A, Sel as S, Skip as K};

/// What a Jellyfin class becomes.
#[derive(Clone, Copy, Debug)]
pub enum To {
    /// A selector (one compound, or a chain such as `.header-brand > .slot:first-child`). Several roles are written as `:is(a, b)`.
    Sel(&'static str),
    /// Same as `Sel`, but the Jellyfin name is also a NOCTURNE name (`.card`): it does not prove the sheet is a Jellyfin theme.
    Same(&'static str),
    /// A wrapper, behaviour or layout class with no element of its own: dropped from a compound or a chain (the descendant relation stays), and a rule that styles it alone is reported.
    Skip(&'static str),
    /// A role NOCTURNE does not have: the selector cannot match and is reported with this reason.
    Never(&'static str),
}

/// Jellyfin class to NOCTURNE. First match wins; the list is searched linearly once per distinct class (a few hundred names).
pub static CLASSES: &[(&str, To)] = &[
    // ---- the root and the page ----
    ("layout-desktop", S(":root")),
    ("layout-tv", S(":root[data-layout=tv]")),
    ("layout-mobile", N("no mobile layout in the desktop app")),
    ("backgroundContainer", S(":root")),
    ("mainAnimatedPage", S(":root")),
    ("libraryPage", S(":root")),
    ("homePage", S(".page-home")),
    ("homeSectionsContainer", S(".page-home")),
    ("itemDetailPage", S(".page-detail")),
    ("detailPageContent", S(".page-detail")),
    ("detailPageWrapperContainer", S(".page-detail")),
    ("pageContainer", S(":root")),
    ("page", S(":root")),
    ("hide", N("hidden state has no element")),
    ("hidden", N("hidden state has no element")),
    ("withBackdrop", S(":root")),
    // ---- the header ----
    ("skinHeader", S(".header")),
    ("skinHeader-withBackground", S(".header")),
    ("skinHeader-blurred", S(".header")),
    ("semiTransparent", S(".header")),
    ("headroom", S(".header")),
    ("headerTop", S(".header-bar")),
    ("headerLeft", S(".header-brand")),
    ("headerMiddle", S(".header-nav")),
    ("headerRight", S(".header-icons")),
    ("headerTabs", S(".header-nav")),
    ("sectionTabs", S(".header-nav")),
    ("emby-tabs-slider", S(".header-nav")),
    ("emby-tab-button", S(".header-link")),
    ("emby-tab-button-active", S(".header-link.active")),
    (
        "headerButton",
        S(":is(.header-search, .header-account, .header-people, .header-filter)"),
    ),
    ("headerSearchButton", S(".header-search")),
    ("headerUserButton", S(".header-account")),
    ("headerBackButton", S(".header-brand > .slot:first-child")),
    ("headerHomeButton", N("the brand is not a button")),
    ("headerCastButton", N("no cast")),
    ("headerSyncButton", N("no sync button")),
    ("headerAudioPlayerButton", N("no audio player button")),
    ("pageTitle", S(".header-name")),
    ("pageTitleWithLogo", S(".header-wordmark")),
    ("pageTitleWithDefaultLogo", S(".header-wordmark")),
    ("headerUserButtonRound", S(".header-account")),
    ("headerUserImage", S(".header-account")),
    ("mainDrawer", N("no side drawer: navigation is the header")),
    (
        "mainDrawer-scrollContainer",
        N("no side drawer: navigation is the header"),
    ),
    (
        "navMenuOption",
        N("no side drawer: navigation is the header"),
    ),
    (
        "navMenuOption-selected",
        N("no side drawer: navigation is the header"),
    ),
    ("appfooter", N("no footer")),
    ("nowPlayingBar", N("no mini player bar")),
    // ---- home sections and rows ----
    ("verticalSection", S(":is(.row, .section)")),
    ("detailVerticalSection", S(".section")),
    ("sections", S(".page-home")),
    ("nextUpSection", S(".section-next-up")),
    ("sectionTitleContainer", S(":is(.row-head, .section-head)")),
    (
        "sectionTitleContainer-cards",
        S(":is(.row-head, .section-head)"),
    ),
    ("sectionTitle", S(":is(.row-title, .section-title)")),
    ("sectionTitle-cards", S(":is(.row-title, .section-title)")),
    (
        "sectionTitleTextButton",
        S(":is(.row-title, .section-title)"),
    ),
    ("sectionTitleButton", N("no title buttons")),
    ("emby-scroller", S(":is(.row-panel, .section-strip)")),
    (
        "emby-scroller-container",
        S(":is(.row-panel, .section-box)"),
    ),
    ("scrollSlider", S(":is(.row-cards, .section-strip)")),
    ("itemsContainer", S(":is(.row-cards, .section-strip)")),
    ("vertical-wrap", S(".row-grid")),
    ("scrollX", S(":is(.row-cards, .section-strip)")),
    ("emby-scrollbuttons", S(".row-arrows")),
    ("emby-scrollbuttons-scrollbutton", S(".row-arrow")),
    ("cardColumn", K("cards are not grouped in columns")),
    // ---- cards ----
    ("card", A(":is(.card, .poster)")),
    ("cardBox", S(":is(.card, .poster)")),
    ("cardBox-bottompadded", S(":is(.card, .poster)")),
    ("visualCardBox", S(":is(.card, .poster)")),
    ("card-withuserdata", S(":is(.card, .poster)")),
    ("card-hoverable", S(":is(.card, .poster)")),
    ("cardScalable", S(":is(.card, .poster)")),
    (
        "cardPadder",
        K("the aspect-ratio spacer: the image has its own size"),
    ),
    ("cardImageContainer", S(":is(.card-image, .poster-art)")),
    ("cardContent", S(":is(.card-image, .poster-art)")),
    ("cardContent-shadow", S(":is(.card-image, .poster-art)")),
    ("cardImage", S(":is(.card-img, .tile-img)")),
    ("coveredImage", K("image fit")),
    ("cardImageIcon", S(":is(.card-initial, .tile-initial)")),
    ("cardFooter", S(":is(.card-caption, .poster-caption)")),
    (
        "cardFooter-transparent",
        S(":is(.card-caption, .poster-caption)"),
    ),
    ("innerCardFooter", S(".card-overlay")),
    ("fullInnerCardFooter", S(".card-overlay")),
    ("innerCardFooterClear", S(".card-overlay")),
    (
        "cardText",
        S(":is(.card-title, .card-meta, .poster-title, .poster-meta)"),
    ),
    ("cardTextCentered", S(":is(.card-title, .card-meta)")),
    ("cardText-first", S(":is(.card-title, .poster-title)")),
    ("cardText-secondary", S(":is(.card-meta, .poster-meta)")),
    ("cardIndicators", S(".card-chips")),
    ("indicator", S(":is(.card-badge, .card-chip)")),
    ("countIndicator", S(".card-badge")),
    ("playedIndicator", S(".card-chip.on")),
    ("videoIndicator", N("no video-type badge")),
    ("timerIndicator", N("no recording badges")),
    ("itemProgressBar", S(":is(.card-progress, .card-bar)")),
    ("itemProgressBarForeground", S(".card-progress-fill")),
    ("cardOverlayContainer", S(".card-ring")),
    ("cardOverlayButton", N("no play overlay buttons on cards")),
    (
        "cardOverlayButton-hover",
        N("no play overlay buttons on cards"),
    ),
    (
        "cardOverlayButton-br",
        N("no play overlay buttons on cards"),
    ),
    (
        "cardOverlayButtonIcon",
        N("no play overlay buttons on cards"),
    ),
    (
        "cardOverlayFab-primary",
        N("no play overlay buttons on cards"),
    ),
    ("cardOverlayFab", N("no play overlay buttons on cards")),
    (
        "portraitCard",
        S(":is(.card, .poster)[data-shape=portrait]"),
    ),
    (
        "overflowPortraitCard",
        S(":is(.card, .poster)[data-shape=portrait]"),
    ),
    (
        "backdropCard",
        S(":is(.card, .poster)[data-shape=backdrop]"),
    ),
    (
        "overflowBackdropCard",
        S(":is(.card, .poster)[data-shape=backdrop]"),
    ),
    ("squareCard", S(":is(.card, .poster)[data-shape=square]")),
    (
        "overflowSquareCard",
        S(":is(.card, .poster)[data-shape=square]"),
    ),
    ("bannerCard", S(":is(.card, .poster)[data-shape=backdrop]")),
    ("personCard", S(":is(.card, .poster)[data-kind=person]")),
    ("chapterCard", N("no chapter cards")),
    ("chapterThumbContainer", N("no chapter cards")),
    ("chapterThumbTextContainer", N("no chapter cards")),
    ("block", K("layout of a card grid")),
    ("itemAction", K("behaviour class")),
    ("show-focus", K("behaviour class")),
    ("show-animation", K("behaviour class")),
    ("lazy", K("image loading state")),
    ("lazy-hidden", N("image loading state")),
    ("blurhash-canvas", N("no blurhash canvas")),
    ("card-hoverable-disabled", K("behaviour class")),
    // ---- the item (detail) page ----
    ("detailPagePrimaryContainer", S(".detail-hero")),
    ("detailPagePrimaryContent", S(".detail-hero")),
    ("detailPageSecondaryContainer", S(".detail-about")),
    ("detailRibbon", S(".detail-top")),
    ("infoWrapper", S(".detail-body")),
    ("detailImageContainer", S(".detail-poster")),
    ("nameContainer", S(".detail-head")),
    ("itemName", S(".detail-title")),
    ("itemName-primary", S(".detail-title")),
    ("itemName-secondary", S(".detail-season")),
    ("infoText", S(".detail-title")),
    ("parentName", N("no parent-name link line")),
    ("parentNameLast", N("no parent-name link line")),
    ("musicParentName", N("no parent-name link line")),
    ("itemMiscInfo", S(".detail-meta")),
    ("itemMiscInfo-primary", S(".detail-meta")),
    ("itemMiscInfo-secondary", S(".detail-chips")),
    ("mediaInfoItem", S(".chip")),
    ("mediaInfoText", S(".chip")),
    ("mediaInfoYear", S(".detail-year")),
    ("mediaInfoTimerType", N("no recording info")),
    ("starRatingContainer", S(".detail-stars")),
    ("mediaInfoOfficialRating", S(".detail-cert")),
    ("mediaInfoCriticRating", S(".detail-rating")),
    ("itemGenres", S(".detail-genres")),
    ("detailLogo", S(".detail-logo")),
    ("itemBackdrop", S(".detail-art")),
    ("backdropContainer", S(":is(.detail-art, .hero-art)")),
    ("backdropImage", S(":is(.detail-backdrop, .hero-img)")),
    ("overview", S(".detail-text")),
    ("overview-text", S(".detail-text")),
    ("detail-clamp-text", S(".detail-summary")),
    ("overview-expand", S(".detail-more")),
    ("tagline", S(".detail-tagline")),
    ("mainDetailButtons", S(".detail-actions")),
    ("detailButton", S(".button")),
    ("detailButton-content", K("button content wrapper")),
    ("detailButton-icon", S(".button-icon")),
    ("detailButton-text", S(".button-label")),
    ("btnPlay", S(".button[data-action=play]")),
    ("btnResume", S(".button[data-action=play]")),
    ("btnShuffle", S(".button[data-action=shuffle]")),
    ("btnPlayAll", S(".button[data-action=play-all]")),
    ("btnPlaystate", S(".button[data-action=watched]")),
    (
        "btnUserRating",
        S(":is(.button[data-action=favourite], .player-btn[data-action=favourite])"),
    ),
    ("btnMoreCommands", S(".button[data-action=more]")),
    (
        "btnPlaystateAndRating",
        S(":is(.button[data-action=watched], .button[data-action=favourite])"),
    ),
    (
        "playstatebutton-icon-played",
        S(".round[data-action=watched].on"),
    ),
    ("btnReplay", N("no replay button")),
    ("btnSplitVersions", S(".button[data-action=versions]")),
    ("btnDownload", N("no download button")),
    ("btnSyncDownload", N("no download button")),
    ("btnShare", N("no share button")),
    ("btnCancelRecording", N("no recording")),
    ("recordingButton", N("no recording")),
    ("recordingFields", N("no recording")),
    (
        "trackSelections",
        N("no track selection fields on the page"),
    ),
    ("selectContainer", N("no select boxes")),
    ("selectLabel", N("no select boxes")),
    ("selectArrowContainer", N("no select boxes")),
    ("detailSection", S(".section")),
    ("detailSectionHeader", S(".section-head")),
    ("detailSectionContent", S(".section-box")),
    ("itemDetailsGroup", S(".detail-facts-table")),
    ("detailsGroupItem", S(".detail-fact")),
    ("itemExternalLinks", S(".detail-tags")),
    ("itemTags", S(".detail-tags")),
    ("collectionItems", S(".section-strip")),
    ("childrenItemsContainer", S(".section-strip")),
    ("castContent", S(".section-strip")),
    ("emby-tabs", S(".detail-tabs")),
    // ---- list items (episodes, tracks) ----
    ("listItem", S(":is(.episode, .entry)")),
    ("listItem-border", K("divider drawn by the row")),
    ("listItem-content", K("row content wrapper")),
    ("listItem-withContentWrapper", K("row content wrapper")),
    ("listItem-focusscale", K("behaviour class")),
    ("listItem-largeImage", K("row size modifier")),
    ("listItemImage", S(":is(.episode-thumb, .entry-still)")),
    (
        "listItemImage-large",
        S(":is(.episode-thumb, .entry-still)"),
    ),
    ("listItemIcon", S(":is(.episode-glyph, .entry-num)")),
    ("listItemBody", S(":is(.episode-body, .entry-info)")),
    (
        "listItemBodyText",
        S(":is(.episode-title, .episode-meta, .entry-title, .entry-meta)"),
    ),
    (
        "listItemBodyText-secondary",
        S(":is(.episode-meta, .entry-meta)"),
    ),
    (
        "listItem-overview",
        S(":is(.episode-overview, .entry-overview)"),
    ),
    ("listItemMediaInfo", S(":is(.episode-meta, .entry-meta)")),
    ("listViewUserDataButtons", S(".episode-actions")),
    ("listItemButton", S(".round")),
    ("listItemImageButton", S(".episode-play")),
    ("listItemIndex", S(".entry-num")),
    ("paperList", K("list container")),
    ("vertical-list", K("list container")),
    // ---- buttons ----
    ("emby-button", S(".button")),
    ("emby-button-foreground", K("button content wrapper")),
    ("button-flat", S(".button")),
    ("button-link", S(".button")),
    ("raised", S(".button")),
    ("fab", S(".button")),
    (
        "button-submit",
        S(":is(.button.primary, .button.button-primary)"),
    ),
    (
        "button-accent",
        S(":is(.button.primary, .button.button-primary)"),
    ),
    (
        "button-accent-flat",
        S(":is(.button.primary, .button.button-primary)"),
    ),
    ("button-delete", N("no delete buttons in the browsing UI")),
    (
        "paper-icon-button-light",
        S(":is(.round, .card-chip, .player-btn)"),
    ),
    ("btnMiniPlay", N("no mini play buttons")),
    ("skip-button", N("no skip-intro button")),
    // ---- the player (OSD) ----
    (
        "videoOsdBottom",
        N("no single bar container (needs `div.player-bar`)"),
    ),
    (
        "videoOsdBottom-maincontrols",
        N("no single bar container (needs `div.player-bar`)"),
    ),
    (
        "osdControls",
        N("no single bar container (needs `div.player-bar`)"),
    ),
    ("osdHeader", N("no OSD header")),
    ("videoOsdHeader", N("no OSD header")),
    ("osdTitle", S(".player-title")),
    ("osdMediaInfo", S(".player-status")),
    ("osdTextContainer", S(".player-info")),
    ("osdPoster", N("no poster in the player bar")),
    ("osdPositionSliderContainer", S(".player-seek")),
    ("sliderContainer", S(":is(.player-seek, .player-vol-track)")),
    ("mdl-slider", S(":is(.player-seek, .player-vol-track)")),
    (
        "mdl-slider-background-lower",
        S(":is(.player-seek, .player-vol-track)"),
    ),
    (
        "mdl-slider-background-flex",
        S(":is(.player-seek, .player-vol-track)"),
    ),
    ("sliderBubble", N("no seek bubble")),
    ("sliderMarker", N("no chapter markers on the seek bar")),
    ("osdTimeText", S(":is(.player-clock, .player-remain)")),
    ("endsAtText", S(".player-ends")),
    ("volumeSliderContainer", S(".player-volume")),
    ("buttons", S(".player-transport")),
    ("btnPause", S(".player-btn[data-action=pause]")),
    (
        "btnPlayPause",
        S(":is(.player-btn[data-action=pause], .player-btn[data-action=play])"),
    ),
    ("btnPreviousTrack", S(".player-btn[data-action=prev]")),
    ("btnNextTrack", S(".player-btn[data-action=next]")),
    ("btnRewind", S(".player-btn[data-action=back]")),
    ("btnFastForward", S(".player-btn[data-action=fwd]")),
    (
        "btnToggleSubtitles",
        S(".player-btn[data-action=subtitles]"),
    ),
    ("btnSubtitles", S(".player-btn[data-action=subtitles]")),
    ("btnAudioTracks", S(".player-btn[data-action=audio]")),
    ("btnMute", S(".player-btn[data-action=mute]")),
    (
        "btnVideoOsdSettings",
        S(".player-btn[data-action=settings]"),
    ),
    ("btnPlaylist", S(".player-btn[data-action=queue]")),
    ("btnFullscreen", N("fullscreen is a window action")),
    ("btnCast", N("no cast")),
    ("btnAirPlay", N("no cast")),
    ("btnStop", N("no stop button")),
    ("btnRepeat", N("no repeat button")),
    ("upNextContainer", N("no up-next card")),
    ("playerStats", N("no stats panel in the bar")),
    (
        "videoOsdBottom-hidden",
        N("no single bar container (needs `div.player-bar`)"),
    ),
    // ---- library views (step E trees carry no jellyfin classes yet) ----
    ("alphaPicker", N("no alphabet picker")),
    ("alphaPickerButton", N("no alphabet picker")),
    ("alphaPicker-fixed-right", N("no alphabet picker")),
    (
        "itemsViewSettingsContainer",
        N("library toolbar classes are not emitted yet"),
    ),
    ("padded-right-withalphapicker", K("alphabet picker spacing")),
    // ---- layout helpers ----
    ("padded-left", K("padding helper")),
    ("padded-right", K("padding helper")),
    ("padded-top", K("padding helper")),
    ("padded-bottom", K("padding helper")),
    ("padded-top-focusscale", K("padding helper")),
    ("padded-bottom-focusscale", K("padding helper")),
    ("padded-bottom-page", K("padding helper")),
    ("padded-top-page", K("padding helper")),
    ("verticalSection-extrabottompadding", K("padding helper")),
    ("noSecondaryNavPage", K("page layout flag")),
    ("withSectionTabs", K("page layout flag")),
    ("noHomeButtonHeader", K("page layout flag")),
    ("focuscontainer-x", K("focus behaviour")),
    ("focuscontainer-y", K("focus behaviour")),
    ("focuscontainer", K("focus behaviour")),
    ("scrollY", K("scroll behaviour")),
    ("flex", K("layout helper")),
    ("flex-grow", K("layout helper")),
    ("material-icons", K("icon font: icons are drawn by the app")),
    ("secondary", K("text role")),
    ("readOnlyContent", K("text role")),
    ("hide-scroll", K("scrollbar flag")),
    ("headroom--unpinned", N("no auto-hiding header")),
    ("headroom--pinned", K("header state")),
    ("headroom--top", K("header state")),
    // ---- forms, dialogs, dashboard: stay hand-written ----
    ("emby-input", N("form fields are hand-written")),
    ("emby-select", N("form fields are hand-written")),
    ("emby-select-withcolor", N("form fields are hand-written")),
    ("emby-textarea", N("form fields are hand-written")),
    ("emby-checkbox", N("form fields are hand-written")),
    ("checkboxOutline", N("form fields are hand-written")),
    ("checkboxContainer", N("form fields are hand-written")),
    ("inputContainer", N("form fields are hand-written")),
    ("emby-slider", N("form fields are hand-written")),
    ("dialog", N("dialogs are hand-written")),
    ("formDialog", N("dialogs are hand-written")),
    ("formDialogHeader", N("dialogs are hand-written")),
    ("formDialogContent", N("dialogs are hand-written")),
    ("formDialogFooter", N("dialogs are hand-written")),
    ("actionSheet", N("menus are hand-written")),
    ("actionSheetMenuItem", N("menus are hand-written")),
    ("toast", N("no toasts")),
    ("dashboardDocument", N("the dashboard is hand-written")),
    ("userPreferencesPage", N("settings are hand-written")),
    ("visualLoginForm", N("sign-in is hand-written")),
    ("manualLoginForm", N("sign-in is hand-written")),
    ("itemSelectionPanel", N("no multi-select panel")),
    ("selectionCommandsPanel", N("no multi-select panel")),
    ("multiSelectCheckboxOutline", N("no multi-select panel")),
    ("searchFields", N("search is a slot")),
    ("searchSuggestionsList", N("search is a slot")),
    ("programGuide", N("no live TV guide")),
    ("programGuideSection", N("no live TV guide")),
    ("programCell", N("no live TV guide")),
    ("programCell-active", N("no live TV guide")),
    ("tvguide", N("no live TV guide")),
    ("guide-date-tab-button", N("no live TV guide")),
    // ---- MUI (jellyfin-web 12 renders the header and some buttons with it) ----
    ("MuiAppBar-root", S(".header")),
    ("MuiToolbar-root", S(".header-bar")),
    (
        "MuiIconButton-root",
        S(":is(.header-search, .header-account, .round)"),
    ),
    ("MuiButtonBase-root", S(".button")),
    ("MuiButton-root", S(".button")),
    ("MuiButton-text", S(".button")),
    ("MuiButton-sizeLarge", S(".button")),
    (
        "MuiButton-containedPrimary",
        S(":is(.button.primary, .button.button-primary)"),
    ),
    (
        "MuiButton-textPrimary",
        S(":is(.button.primary, .button.button-primary)"),
    ),
    ("MuiButtonGroup-root", S(".detail-actions")),
    ("MuiChip-root", S(".chip")),
    ("MuiTab-root", S(":is(.header-link, .tab)")),
    ("Mui-selected", S(".active")),
    ("MuiAvatar-root", S(".header-account")),
    ("MuiBox-root", K("layout helper")),
    ("MuiStack-root", K("layout helper")),
    ("MuiTabs-indicator", N("no tab underline element")),
    ("MuiPopover-paper", N("menus are hand-written")),
    ("MuiMenu-paper", N("menus are hand-written")),
    ("MuiMenuItem-root", N("menus are hand-written")),
    (
        "MuiDrawer-paper",
        N("no side drawer: navigation is the header"),
    ),
    ("MuiDialog-paper", N("dialogs are hand-written")),
    ("MuiPaper-root", N("menus are hand-written")),
    ("MuiList-root", N("menus are hand-written")),
    ("MuiList-subheader", N("menus are hand-written")),
    ("MuiListItemButton-root", N("menus are hand-written")),
    ("MuiListItemIcon-root", N("menus are hand-written")),
    ("MuiCheckbox-root", N("form fields are hand-written")),
    ("Mui-checked", N("form fields are hand-written")),
    ("MuiFilledInput-root", N("form fields are hand-written")),
    ("MuiLinearProgress-root", N("no linear progress widget")),
    // ---- more names met in community themes ----
    ("homeLibraryButton", S(".section-tiles :is(.card, .poster)")),
    ("groupedCard", S(":is(.card, .poster)")),
    ("defaultCardBackground", S(":is(.card-image, .poster-art)")),
    ("defaultCardBackground1", S(":is(.card-image, .poster-art)")),
    ("defaultCardBackground2", S(":is(.card-image, .poster-art)")),
    ("defaultCardBackground3", S(":is(.card-image, .poster-art)")),
    ("defaultCardBackground4", S(":is(.card-image, .poster-art)")),
    ("defaultCardBackground5", S(":is(.card-image, .poster-art)")),
    ("textActionButton", K("link inside a card caption")),
    ("itemDetailImage", S(".detail-poster")),
    ("collectionItemsContainer", S(".section-strip")),
    ("collapseContent", S(".section-box")),
    ("mediaInfoContent", S(".detail-meta")),
    ("label", S(".detail-fact-key")),
    ("starIcon", S(":is(.star-full, .star-half, .star-empty)")),
    ("listItem-button", S(".round")),
    (
        "listItem-bottomoverview",
        S(":is(.episode-overview, .entry-overview)"),
    ),
    ("startTimeText", S(".player-clock")),
    ("endTimeText", S(".player-remain")),
    (
        "mdl-slider-background-upper",
        S(":is(.player-seek, .player-vol-track)"),
    ),
    (
        "mdl-slider-hoverthumb",
        S(":is(.player-seek, .player-vol-track)"),
    ),
    ("showing-osd", S(":root[data-screen=player]")),
    ("videoOsd-visible", S(":root[data-screen=player]")),
    ("osd-visible", S(":root[data-screen=player]")),
    ("iconOsd", N("no volume or brightness popup")),
    ("iconOsdProgressInner", N("no volume or brightness popup")),
    ("upNextDialog", N("no up-next card")),
    ("upNextDialog-countdownText", N("no up-next card")),
    ("upNextDialog-buttons", N("no up-next card")),
    ("upNextDialog-button", N("no up-next card")),
    ("btnFilter", N("library toolbar: step E")),
    ("btnSort", N("library toolbar: step E")),
    ("btnSelectView", N("library toolbar: step E")),
    ("btnNextPage", N("library toolbar: step E")),
    ("btnPreviousPage", N("library toolbar: step E")),
    ("listPaging", N("library toolbar: step E")),
    ("mediaSourceIndicator", N("no media source badge")),
    ("chapterCardImageContainer", N("no chapter cards")),
    ("chapterThumb", N("no chapter cards")),
    ("chapterThumbText", N("no chapter cards")),
    ("nowPlayingPage", N("no now-playing page")),
    ("nowPlayingPageImage", N("no now-playing page")),
    ("nowPlayingInfoContainer", N("no now-playing page")),
    ("settingsPage", N("settings are hand-written")),
    ("settingsContainer", N("settings are hand-written")),
    ("adminPage", N("the dashboard is hand-written")),
    ("dashboardSection", N("the dashboard is hand-written")),
    ("activeSession", N("the dashboard is hand-written")),
    ("serverInfo", N("the dashboard is hand-written")),
    ("dialogContainer", N("dialogs are hand-written")),
    ("dialogContentInner", N("dialogs are hand-written")),
    ("formDialogHeaderTitle", N("dialogs are hand-written")),
    ("actionSheetContent", N("menus are hand-written")),
    ("subtitleEditorDialog", N("dialogs are hand-written")),
    ("subtitleappearance-preview", N("dialogs are hand-written")),
    ("inputLabelFocused", N("form fields are hand-written")),
    ("selectLabelFocused", N("form fields are hand-written")),
    ("textareaLabelFocused", N("form fields are hand-written")),
    ("mdl-radio", N("form fields are hand-written")),
    ("mdl-radio__focus-circle", N("form fields are hand-written")),
    ("docspinner", N("no page spinner")),
    ("mdl-spinner__layer-1", N("no page spinner")),
    ("mdl-spinner__layer-2", N("no page spinner")),
    ("mdl-spinner__layer-3", N("no page spinner")),
    ("mdl-spinner__layer-4", N("no page spinner")),
    ("progressring-spiner", N("no page spinner")),
    ("blurhashed", K("image loading state")),
    ("noBackdropTransparency", K("page flag")),
    ("buttonActive", S(".active")),
    ("clearLink", K("link style")),
    ("overview-controls", K("overview wrapper")),
    ("backgroundProgress", N("no background progress bar")),
    ("playbackProgress", N("no background progress bar")),
    ("videoPlayerContainer", S(":root[data-screen=player]")),
];

/// Is this class one of NOCTURNE's own (the stage-2 vocabulary of the tree builders)? Such a class passes through unchanged, so a sheet written for NOCTURNE keeps working when it is layered after a Jellyfin theme.
pub fn is_native_class(c: &str) -> bool {
    const PREFIX: &[&str] = &[
        "card-",
        "hero-",
        "header-",
        "row-",
        "detail-",
        "section-",
        "page-",
        "player-",
        "poster-",
        "episode-",
        "entry-",
        "button-",
        "chip-",
        "band-",
        "tab-",
        "star-",
        "progress-",
        "skeleton-",
        "tile-",
        "slot-",
        "mark-",
    ];
    const EXACT: &[&str] = &[
        "card", "detail", "tab", "round", "active", "on", "dim", "keep", "lifted", "primary",
        "accent", "labeled", "flat", "slot", "section", "row", "header", "hero", "chip", "tile",
        "poster", "episode", "entry", "button", "band", "skeleton", "progress",
    ];
    EXACT.contains(&c) || PREFIX.iter().any(|p| c.starts_with(p))
}

/// Jellyfin or common community custom properties to NOCTURNE tokens. A theme that sets the left name also sets the right ones (same value); a theme that only reads the left name gets the NOCTURNE value as its default.
pub static VARS: &[(&str, &[&str])] = &[
    ("--jf-palette-primary-main", &["--nocturne-accent"]),
    ("--jf-palette-secondary-main", &["--nocturne-accent"]),
    ("--jf-palette-primary-light", &["--nocturne-accent-text"]),
    ("--jf-palette-primary-dark", &["--nocturne-accent-hover"]),
    (
        "--jf-palette-primary-contrastText",
        &["--nocturne-on-accent"],
    ),
    (
        "--jf-palette-secondary-contrastText",
        &["--nocturne-on-accent"],
    ),
    ("--jf-palette-background-default", &["--nocturne-frame"]),
    ("--jf-palette-background-paper", &["--nocturne-surface"]),
    ("--jf-palette-AppBar-defaultBg", &["--nocturne-header-bg"]),
    (
        "--jf-palette-AppBar-transparentBg",
        &["--nocturne-header-bg"],
    ),
    ("--jf-palette-text-primary", &["--nocturne-text"]),
    (
        "--jf-palette-text-secondary",
        &["--nocturne-text-secondary"],
    ),
    ("--jf-palette-text-disabled", &["--nocturne-text-tertiary"]),
    ("--jf-palette-divider", &["--nocturne-hairline"]),
    (
        "--jf-palette-Button-inheritContainedBg",
        &["--nocturne-raised"],
    ),
    ("--jf-card-borderRadius", &["--nocturne-r-card"]),
    ("--accentColor", &["--nocturne-accent"]),
    ("--accent-color", &["--nocturne-accent"]),
    ("--theme-primary-color", &["--nocturne-accent"]),
    ("--theme-accent-color", &["--nocturne-accent"]),
    ("--primary-accent-color", &["--nocturne-accent"]),
    ("--textColor", &["--nocturne-text"]),
    ("--dimTextColor", &["--nocturne-text-secondary"]),
    ("--headerBackground", &["--nocturne-header-bg"]),
    ("--borderColor", &["--nocturne-edge"]),
];

/// Attribute selectors on Jellyfin data attributes: `[data-type=Movie]` and friends. `None` means no counterpart.
pub fn attr(name: &str, op: &str, value: &str) -> Option<String> {
    let v = value.trim_matches(|c| c == '"' || c == '\'');
    match (name, op) {
        ("data-type", "=") => {
            let kind = match v.to_ascii_lowercase().as_str() {
                "movie" => "movie",
                "series" => "series",
                "season" => "season",
                "episode" => "episode",
                "photo" => "photo",
                "book" => "book",
                "person" => "person",
                "boxset" | "folder" | "collectionfolder" | "userview" | "playlist" => "folder",
                _ => return None,
            };
            Some(format!("[data-kind={kind}]"))
        }
        ("data-mediatype", "=") => match v.to_ascii_lowercase().as_str() {
            "video" => Some(":is([data-kind=movie], [data-kind=episode])".into()),
            "photo" => Some("[data-kind=photo]".into()),
            "book" => Some("[data-kind=book]".into()),
            _ => None,
        },
        ("data-isfolder", "=") => match v {
            "true" => {
                Some(":is([data-kind=folder], [data-kind=series], [data-kind=season])".into())
            }
            "false" => Some(
                ":is([data-kind=movie], [data-kind=episode], [data-kind=photo], [data-kind=book])"
                    .into(),
            ),
            _ => None,
        },
        ("data-played", "=") => Some(format!("[data-watched={v}]")),
        _ => None,
    }
}

/// Data attributes NOCTURNE emits itself (passed through).
pub fn is_native_attr(n: &str) -> bool {
    matches!(
        n,
        "data-kind"
            | "data-source"
            | "data-watched"
            | "data-screen"
            | "data-section"
            | "data-region"
            | "data-expanded"
            | "data-action"
            | "data-video"
            | "data-shape"
            | "data-layout"
    )
}

/// Tags that exist in the trees (a Jellyfin sheet's `div`, `a`, `span` rules mean the same there).
pub fn keep_tag(t: &str) -> bool {
    matches!(
        t,
        "a" | "div" | "span" | "section" | "nav" | "header" | "i" | "img" | "page"
    )
}

/// Pseudo-classes the engine supports, passed through.
pub fn pass_pseudo(n: &str) -> bool {
    matches!(
        n,
        "hover"
            | "focus"
            | "focus-visible"
            | "focus-within"
            | "active"
            | "first-child"
            | "last-child"
            | "only-child"
            | "nth-child"
            | "nth-last-child"
            | "first-of-type"
            | "last-of-type"
            | "only-of-type"
            | "nth-of-type"
            | "nth-last-of-type"
            | "empty"
            | "root"
            | "before"
            | "after"
    )
}

/// Pseudo-classes that are always true for NOCTURNE's elements (dropped).
pub fn always_pseudo(n: &str) -> bool {
    matches!(
        n,
        "enabled" | "any-link" | "link" | "defined" | "-webkit-any-link"
    )
}

/// Jellyfin element ids: the page roots and the item page's collapsible sections.
pub fn id(n: &str) -> Option<To> {
    Some(match n {
        "itemDetailPage" => S(".page-detail"),
        "indexPage" | "homePage" => S(".page-home"),
        "videoOsdPage" => S(":root[data-screen=player]"),
        "searchPage" => S(":root[data-screen=search]"),
        "moviesPage" | "tvshowsPage" | "musicPage" | "booksPage" | "collectionsPage"
        | "playlistsPage" => S(":root[data-screen=library]"),
        "reactRoot" | "reactRoot2" => S(":root"),
        "castCollapsible" | "castContent" | "guestCastCollapsible" | "guestCastContent" => {
            S(".section[data-section=cast]")
        }
        "similarCollapsible" => S(".section[data-section=similar]"),
        "specialsCollapsible" | "musicVideosCollapsible" | "additionalPartsCollapsible" => {
            S(".section[data-section=extras]")
        }
        "scenesCollapsible" => S(".section[data-section=chapters]"),
        "childrenContent" | "childrenCollapsible" | "listChildrenCollapsible" => {
            S(":is(.section-list, .section[data-section=seasons])")
        }
        "nextUpContent" | "nextUpCollapsible" => S(".section[data-section=next-up]"),
        "skipIntro" => N("no skip-intro button"),
        "searchTextInput" => N("search is a slot"),
        "slides-container" => N("no home slideshow"),
        n if n.ends_with("Page") => {
            N("pages that stay hand-written (sign-in, settings, dashboard, live TV)")
        }
        n if n.starts_with("btn") => N("dashboard and dialog buttons are hand-written"),
        _ => return None,
    })
}
