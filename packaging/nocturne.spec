# RPM for Fedora, built in a fedora container by scripts/package-rpm.sh (Source0 is the tree tarball it makes).
# libmpv is dlopen()ed, so rpm cannot detect it: every run-time library is listed by hand (docs/PACKAGING.md).
%global debug_package %{nil}
%global __os_install_post %{nil}

Name:           nocturne
Version:        %{?pkgver}%{!?pkgver:0.3.0}
Release:        1%{?dist}
Summary:        Native media center for local folders and Jellyfin servers
License:        GPL-3.0-or-later
URL:            https://nocturne.ifsvivek.in
Source0:        %{name}-%{version}.tar.gz
BuildRequires:  gcc, gcc-c++, make, pkgconf-pkg-config, dbus-devel, systemd-devel, libglvnd-devel, curl

# mpv-libs provides libmpv.so.2; on Fedora it comes from RPM Fusion (free).
Requires:       mpv-libs
Requires:       systemd-libs
Requires:       libglvnd-egl
Requires:       vulkan-loader
Requires:       libwayland-client
Requires:       libwayland-egl
Requires:       libxkbcommon
Requires:       libxkbcommon-x11
Requires:       libX11
Requires:       libX11-xcb
Requires:       libXcursor
Requires:       libXi
Requires:       libxcb
Requires:       dbus-libs
Recommends:     mesa-vulkan-drivers
Recommends:     mpv
Recommends:     xdg-desktop-portal
Recommends:     google-noto-sans-fonts
Recommends:     gnome-keyring

%description
One cinematic, searchable library over local media and any number of Jellyfin
servers, playing video inside the app window through libmpv. Not affiliated
with the Jellyfin project.

%prep
%autosetup

%build
# rustup toolchain (Fedora's rust may lag the MSRV); CARGO_HOME/TARGET are set by the build script.
export PATH="$CARGO_HOME/bin:$PATH"
RUSTFLAGS="--remap-path-prefix=$CARGO_HOME=/cargo --remap-path-prefix=$PWD=/src" cargo build --release --locked -p nocturne-app

%install
install -Dm755 "$CARGO_TARGET_DIR/release/nocturne" %{buildroot}%{_bindir}/nocturne
install -Dm644 packaging/io.github.ifsvivek.nocturne.desktop -t %{buildroot}%{_datadir}/applications
for s in 48 128 256 512; do
  install -Dm644 packaging/icons/nocturne-$s.png %{buildroot}%{_datadir}/icons/hicolor/${s}x${s}/apps/nocturne.png
done
install -Dm644 packaging/nocturne.svg -t %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -Dm644 packaging/io.github.ifsvivek.nocturne.metainfo.xml -t %{buildroot}%{_datadir}/metainfo
install -Dm644 LICENSE THIRD_PARTY_NOTICES.md -t %{buildroot}%{_datadir}/licenses/%{name}

%files
%license %{_datadir}/licenses/%{name}/LICENSE
%{_datadir}/licenses/%{name}/THIRD_PARTY_NOTICES.md
%{_bindir}/nocturne
%{_datadir}/applications/io.github.ifsvivek.nocturne.desktop
%{_datadir}/icons/hicolor/*/apps/nocturne.*
%{_datadir}/metainfo/io.github.ifsvivek.nocturne.metainfo.xml
