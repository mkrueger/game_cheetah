use std::collections::BTreeSet;

fn message_ids(ftl: &str) -> BTreeSet<&str> {
    ftl.lines()
        .filter_map(|line| {
            if line.starts_with(char::is_whitespace) || line.starts_with('#') {
                return None;
            }

            let (id, _) = line.split_once('=')?;
            let id = id.trim();
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return None;
            }
            Some(id)
        })
        .collect()
}

#[test]
fn english_and_german_locales_have_the_same_message_ids() {
    let english = message_ids(include_str!("../i18n/en/game_cheetah.ftl"));
    let german = message_ids(include_str!("../i18n/de/game_cheetah.ftl"));

    let missing_in_german: Vec<_> = english.difference(&german).copied().collect();
    let extra_in_german: Vec<_> = german.difference(&english).copied().collect();

    assert!(
        missing_in_german.is_empty() && extra_in_german.is_empty(),
        "i18n key mismatch\nmissing in German: {missing_in_german:#?}\nextra in German: {extra_in_german:#?}"
    );
}
