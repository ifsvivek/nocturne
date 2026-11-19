//! Shortcut reference shown by the help overlay (`?`); shared by the front ends.

/// (heading, [(keys, what it does)]). The player has its own `?` panel with playback keys.
pub const SHORTCUTS: [(&str, &[(&str, &str)]); 7] = [
    (
        "Everywhere",
        &[
            ("Arrow keys", "Move the focus"),
            ("Enter", "Open or activate"),
            ("Esc / Backspace", "Go back, close a panel"),
            ("Alt + Left / Right", "Back / forward"),
            ("Ctrl + K or /", "Search"),
            ("a", "Account menu"),
            (
                "g",
                "Watch together: create, join or leave a SyncPlay group",
            ),
            ("?", "This help"),
            ("Ctrl + Shift + 0", "Safe look: the built-in theme, at once"),
        ],
    ),
    (
        "Library",
        &[
            ("o", "Sort"),
            ("l", "Filters"),
            ("v", "Poster / thumb / list view"),
            ("Tab", "Next tab"),
            ("w", "Mark watched / unwatched"),
            ("f", "Favourite"),
        ],
    ),
    (
        "Photos",
        &[
            ("Left / Right  p n", "Previous / next photo"),
            ("+ -  Up / Down", "Zoom in / out"),
            ("0", "Reset zoom"),
            ("Esc", "Close the viewer"),
        ],
    ),
    (
        "Playlists and music queue",
        &[
            ("n", "New playlist (on the Playlists page)"),
            (
                "x  [  ]",
                "Remove / move up / move down the selected entry of your playlist",
            ),
            ("More ...", "Add to playlist, Play next, Add to queue"),
            ("Account menu", "Play or clear the music queue"),
            (
                "u (player)",
                "Queue: Enter play, [ ] move, X remove, Z shuffle, R repeat",
            ),
        ],
    ),
    (
        "Admin",
        &[
            ("Up / Down", "Choose a section or row"),
            ("Left / Right", "Move between buttons, back to the sidebar"),
            ("Enter", "Press the focused button"),
            ("r", "Reload this page"),
            ("Esc", "Leave Admin"),
        ],
    ),
    (
        "Dialogs",
        &[
            ("Tab / Up / Down", "Next / previous field or button"),
            ("Enter", "Next field, or press the focused button"),
            ("Esc", "Cancel"),
        ],
    ),
    (
        "Player",
        &[("Space", "Play / pause"), ("?", "All playback keys")],
    ),
];
