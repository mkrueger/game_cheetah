# Generated with rust2rpm 28 and adapted for Game Cheetah.
%bcond check 0

%global cargo_install_lib 0
%global crate game-cheetah

Name:           rust-game-cheetah
Version:        0.7.3
Release:        1%{?dist}
Summary:        Memory scanner, editor, and game trainer

License:        Apache-2.0
URL:            https://crates.io/crates/game-cheetah
Source0:        %{crates_source}
Source1:        game-cheetah-0.7.3-vendor.tar.xz
Source2:        io.github.mkrueger.game_cheetah.metainfo.xml

BuildRequires:  cargo-rpm-macros >= 26
BuildRequires:  appstream
BuildRequires:  desktop-file-utils

%global _description %{expand:
High-performance memory scanner/editor and game trainer for Linux,
Windows, and macOS.}

%description %{_description}

%package     -n %{crate}
Summary:        %{summary}
License:        Apache-2.0
# LICENSE.dependencies contains the complete bundled dependency license report.
Requires:       xdg-utils

%description -n %{crate} %{_description}

%files       -n %{crate}
%license LICENSE
%license build/license.rtf
%license LICENSE.dependencies
%license cargo-vendor.txt
%doc CHANGELOG.md
%doc README.md
%{_bindir}/game-cheetah
%{_datadir}/applications/game-cheetah.desktop
%{_datadir}/icons/hicolor/128x128/apps/game-cheetah.png
%{_datadir}/icons/hicolor/256x256/apps/game-cheetah.png
%{_metainfodir}/io.github.mkrueger.game_cheetah.metainfo.xml

%prep
%autosetup -n %{crate}-%{version} -p1 -a1
%cargo_prep -v vendor

%build
%cargo_build
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies
%{cargo_vendor_manifest}

%install
%cargo_install
install -Dpm0644 build/linux/game-cheetah.desktop \
  %{buildroot}%{_datadir}/applications/game-cheetah.desktop
install -Dpm0644 build/linux/128x128.png \
  %{buildroot}%{_datadir}/icons/hicolor/128x128/apps/game-cheetah.png
install -Dpm0644 build/linux/256x256.png \
  %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/game-cheetah.png
install -Dpm0644 %{SOURCE2} \
  %{buildroot}%{_metainfodir}/io.github.mkrueger.game_cheetah.metainfo.xml

%check
desktop-file-validate build/linux/game-cheetah.desktop
appstreamcli validate --no-net %{SOURCE2}
%if %{with check}
%cargo_test
%endif

%changelog
* Mon Sep 07 2026 Mike Krüger <mkrueger@posteo.de> - 0.7.3-1
- Initial COPR package