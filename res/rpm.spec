Name:       hdobbydesk
Version:    1.4.9
Release:    0
Summary:    RPM package
License:    GPL-3.0
Vendor:     hdobbydesk <info@hdobbydesk.com>
Requires:   gtk3 libxcb libXfixes alsa-lib libva2 pam gstreamer1-plugins-base
Recommends: libayatana-appindicator-gtk3 libxdo

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

%global __python %{__python3}

%install
mkdir -p %{buildroot}/usr/bin/
mkdir -p %{buildroot}/usr/share/hdobbydesk/
mkdir -p %{buildroot}/usr/share/hdobbydesk/files/
mkdir -p %{buildroot}/usr/share/icons/hicolor/256x256/apps/
mkdir -p %{buildroot}/usr/share/icons/hicolor/scalable/apps/
install -m 755 $HBB/target/release/hdobbydesk %{buildroot}/usr/bin/hdobbydesk
install $HBB/libsciter-gtk.so %{buildroot}/usr/share/hdobbydesk/libsciter-gtk.so
install $HBB/res/hdobbydesk.service %{buildroot}/usr/share/hdobbydesk/files/
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/hdobbydesk.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/hdobbydesk.svg
install $HBB/res/hdobbydesk.desktop %{buildroot}/usr/share/hdobbydesk/files/
install $HBB/res/hdobbydesk-link.desktop %{buildroot}/usr/share/hdobbydesk/files/

%files
/usr/bin/hdobbydesk
/usr/share/hdobbydesk/libsciter-gtk.so
/usr/share/hdobbydesk/files/hdobbydesk.service
/usr/share/icons/hicolor/256x256/apps/hdobbydesk.png
/usr/share/icons/hicolor/scalable/apps/hdobbydesk.svg
/usr/share/hdobbydesk/files/hdobbydesk.desktop
/usr/share/hdobbydesk/files/hdobbydesk-link.desktop
/usr/share/hdobbydesk/files/__pycache__/*

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
    rm /usr/share/applications/hdobbydesk.desktop || true
    rm /usr/share/applications/hdobbydesk-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
