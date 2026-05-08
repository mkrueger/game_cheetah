fn plist_value(plist: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let key_pos = plist.find(&needle)?;
    let after_key = &plist[key_pos + needle.len()..];
    let string_start = after_key.find("<string>")? + "<string>".len();
    let after_start = &after_key[string_start..];
    let string_end = after_start.find("</string>")?;
    Some(after_start[..string_end].to_owned())
}

#[test]
fn mac_bundle_plist_matches_packaged_binary() {
    let plist = include_str!("../build/mac/Info.plist");

    assert_eq!(plist_value(plist, "CFBundleExecutable").as_deref(), Some(env!("CARGO_PKG_NAME")));
    assert_eq!(plist_value(plist, "CFBundleShortVersionString").as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(plist_value(plist, "CFBundleVersion").as_deref(), Some(env!("CARGO_PKG_VERSION")));
}
