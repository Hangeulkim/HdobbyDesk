Name:       hdobbydesk
Version:    1.4.9
Release:    0
Summary:    RPM package
License:    GPL-3.0
Vendor:     hdobbydesk <info@hdobbydesk.com>
Requires:   gtk3 libxcb1 libXfixes3 alsa-utils libXtst6 libva2 pam gstreamer-plugins-base gstreamer-plugin-pipewire
Recommends: libayatana-appindicator3-1 xdotool
Provides:   libdesktop_drop_plugin.so()(64bit), libdesktop_multi_window_plugin.so()(64bit), libfile_selector_linux_plugin.so()(64bit), libflutter_custom_cursor_plugin.so()(64bit), libflutter_linux_gtk.so()(64bit), libscreen_retriever_plugin.so()(64bit), libtray_manager_plugin.so()(64bit), liburl_launcher_linux_plugin.so()(64bit), libwindow_manager_plugin.so()(64bit), libwindow_size_plugin.so()(64bit), libtexture_rgba_renderer_plugin.so()(64bit)

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

# %global __python %{__python3}

%install

mkdir -p "%{buildroot}/usr/share/hdobbydesk" && cp -r ${HBB}/flutter/build/linux/x64/release/bundle/* -t "%{buildroot}/usr/share/hdobbydesk"
mkdir -p "%{buildroot}/usr/bin"
install -Dm 644 $HBB/res/hdobbydesk.service -t "%{buildroot}/usr/share/hdobbydesk/files"
install -Dm 644 $HBB/res/hdobbydesk.desktop -t "%{buildroot}/usr/share/hdobbydesk/files"
install -Dm 644 $HBB/res/hdobbydesk-link.desktop -t "%{buildroot}/usr/share/hdobbydesk/files"
install -Dm 644 $HBB/res/128x128@2x.png "%{buildroot}/usr/share/icons/hicolor/256x256/apps/hdobbydesk.png"
install -Dm 644 $HBB/res/scalable.svg "%{buildroot}/usr/share/icons/hicolor/scalable/apps/hdobbydesk.svg"

%files
/usr/share/hdobbydesk/*
/usr/share/hdobbydesk/files/hdobbydesk.service
/usr/share/icons/hicolor/256x256/apps/hdobbydesk.png
/usr/share/icons/hicolor/scalable/apps/hdobbydesk.svg
/usr/share/hdobbydesk/files/hdobbydesk.desktop
/usr/share/hdobbydesk/files/hdobbydesk-link.desktop

%changelog
# let's skip this for now

%pre
# can do something for centos7
case "$1" in
  1)
    # for install
  ;;
  2)
    # for upgrade
    systemctl stop hdobbydesk || true
  ;;
esac

%post
cp /usr/share/hdobbydesk/files/hdobbydesk.service /etc/systemd/system/hdobbydesk.service
cp /usr/share/hdobbydesk/files/hdobbydesk.desktop /usr/share/applications/
cp /usr/share/hdobbydesk/files/hdobbydesk-link.desktop /usr/share/applications/
ln -sf /usr/share/hdobbydesk/hdobbydesk /usr/bin/hdobbydesk
systemctl daemon-reload
systemctl enable hdobbydesk
systemctl start hdobbydesk
update-desktop-database

%preun
case "$1" in
  0)
    # for uninstall
    systemctl stop hdobbydesk || true
    systemctl disable hdobbydesk || true
    rm /etc/systemd/system/hdobbydesk.service || true
  ;;
  1)
    # for upgrade
  ;;
esac

%postun
case "$1" in
  0)
    # for uninstall
    rm /usr/bin/hdobbydesk || true
    rmdir /usr/lib/hdobbydesk || true
    rmdir /usr/local/hdobbydesk || true
    rmdir /usr/share/hdobbydesk || true
    rm /usr/share/applications/hdobbydesk.desktop || true
    rm /usr/share/applications/hdobbydesk-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
    rmdir /usr/lib/hdobbydesk || true
    rmdir /usr/local/hdobbydesk || true
  ;;
esac
