const APP_ID: &str = "io.github.mkrueger.game_cheetah";
const METAINFO: &str = include_str!("../build/linux/io.github.mkrueger.game_cheetah.metainfo.xml");
const DESKTOP_FILE: &str = include_str!("../build/linux/game-cheetah.desktop");

#[test]
fn metainfo_identifies_the_installed_desktop_application() {
    assert!(METAINFO.contains(&format!("<id>{APP_ID}</id>")));
    assert!(METAINFO.contains("<launchable type=\"desktop-id\">game-cheetah.desktop</launchable>"));
    assert!(DESKTOP_FILE.contains("Exec=/usr/bin/game-cheetah"));
    assert!(DESKTOP_FILE.contains("Icon=game-cheetah"));
}

#[test]
fn linux_packages_install_the_metainfo_file() {
    let cargo_toml = include_str!("../Cargo.toml");
    let arch_builder = include_str!("../build_arch.sh");
    let fedora_spec = include_str!("../build/fedora/game-cheetah.spec");
    let release_workflow = include_str!("../.github/workflows/release.yml");
    let filename = format!("{APP_ID}.metainfo.xml");

    assert!(cargo_toml.contains(&filename), "Debian package must install {filename}");
    assert!(arch_builder.contains(&filename), "Arch package must install {filename}");
    assert!(fedora_spec.contains(&filename), "Fedora package must install {filename}");
    assert!(release_workflow.contains(&filename), "AppImage must contain {filename}");
}
