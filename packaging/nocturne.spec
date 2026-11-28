# RPM for Fedora, built in a fedora container by scripts/package-rpm.sh (Source0 is the tree tarball it makes). The binaries are prebuilt on debian:stable by scripts/build-linux-bin.sh and passed as %{binpath} (app), %{gwpath} (gateway companion) and %{srvpath} (headless server).
# libmpv is dlopen()ed, so rpm cannot detect it: every run-time library is listed by hand (docs/PACKAGING.md).
%global debug_package %{nil}
%global __os_install_post %{nil}

Name:           nocturne
Version:        %{?pkgver}%{!?pkgver:0.7.0}
Release:        1%{?dist}
Summary:        Native media center for local folders and Jellyfin servers
License:        GPL-3.0-or-later
URL:            https://nocturne.ifsvivek.in
Source0:        %{name}-%{version}.tar.gz

# mpv-libs provides libmpv.so.2; on Fedora it comes from RPM Fusion (free).
Requires:       nocturne-server = %{version}-%{release}
Requires:       mpv-libs
Requires:       systemd-libs
Requires:       libglvnd-egl
Requires:       vulkan-loader
Requires:       libwayland-client
Requires:       libwayland-egl
Requires:       libxkbcommon
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

%package server
Summary:        NOCTURNE headless media server
# Linked libraries (glibc, libgcc_s, libdbus-1) are found by rpm's automatic requirements; nothing is dlopen()ed.
Recommends:     gnome-keyring
Suggests:       cloudflared
Suggests:       gvfs-smb
Suggests:       nfs-utils

%description server
Library manager, downloads, requests, gateway and storage modules behind one
REST and WebSocket API, with a web dashboard. Used by the NOCTURNE desktop
app; runs alone on a headless machine.

%prep
%autosetup

%install
install -Dm755 %{binpath} %{buildroot}%{_bindir}/nocturne
install -Dm755 %{gwpath} %{buildroot}%{_bindir}/nocturne-gateway
install -Dm755 %{srvpath} %{buildroot}%{_bindir}/nocturne-server
install -Dm644 packaging/io.github.ifsvivek.nocturne.desktop -t %{buildroot}%{_datadir}/applications
for s in 48 128 256 512; do
  install -Dm644 packaging/icons/nocturne-$s.png %{buildroot}%{_datadir}/icons/hicolor/${s}x${s}/apps/nocturne.png
done
install -Dm644 packaging/nocturne.svg -t %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -Dm644 packaging/io.github.ifsvivek.nocturne.metainfo.xml -t %{buildroot}%{_datadir}/metainfo
install -Dm644 LICENSE THIRD_PARTY_NOTICES.md -t %{buildroot}%{_datadir}/licenses/%{name}
install -Dm644 LICENSE THIRD_PARTY_NOTICES.md -t %{buildroot}%{_datadir}/licenses/%{name}-server

%files
%license %{_datadir}/licenses/%{name}/LICENSE
%{_datadir}/licenses/%{name}/THIRD_PARTY_NOTICES.md
%{_bindir}/nocturne
%{_bindir}/nocturne-gateway
%{_datadir}/applications/io.github.ifsvivek.nocturne.desktop
%{_datadir}/icons/hicolor/*/apps/nocturne.*
%{_datadir}/metainfo/io.github.ifsvivek.nocturne.metainfo.xml

%files server
%license %{_datadir}/licenses/%{name}-server/LICENSE
%{_datadir}/licenses/%{name}-server/THIRD_PARTY_NOTICES.md
%{_bindir}/nocturne-server
