//! The Windows icon embedded by build.rs and the WiX installer.

const APP_ICO: &[u8] = include_bytes!("../build/windows/app.ico");
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

struct Entry {
    size: u32,
    bit_count: u16,
    data: &'static [u8],
}

fn entries() -> Vec<Entry> {
    let u16_at = |offset: usize| u16::from_le_bytes([APP_ICO[offset], APP_ICO[offset + 1]]);
    let u32_at = |offset: usize| u32::from_le_bytes(APP_ICO[offset..offset + 4].try_into().unwrap());
    assert_eq!((u16_at(0), u16_at(2)), (0, 1), "app.ico must be an icon file");
    (0..usize::from(u16_at(4)))
        .map(|index| {
            let entry = 6 + index * 16;
            let (width, height) = (u32::from(APP_ICO[entry]), u32::from(APP_ICO[entry + 1]));
            assert_eq!(width, height, "Icon entries must be square");
            assert_eq!(u16_at(entry + 4), 1, "Icon entries must have one color plane");
            let (length, offset) = (u32_at(entry + 8) as usize, u32_at(entry + 12) as usize);
            assert!(offset + length <= APP_ICO.len(), "Icon entry {index} exceeds the file");
            Entry {
                size: if width == 0 { 256 } else { width },
                bit_count: u16_at(entry + 6),
                data: &APP_ICO[offset..offset + length],
            }
        })
        .collect()
}

#[test]
fn windows_icon_has_all_sizes_with_a_compressed_256_px_entry() {
    let entries = entries();
    let sizes: Vec<_> = entries.iter().map(|entry| entry.size).collect();
    assert_eq!(sizes, [256, 128, 64, 48, 32, 24, 16]);

    for entry in &entries {
        assert_eq!(entry.bit_count, 32, "{} px entry must keep its alpha channel", entry.size);
        if entry.size == 256 {
            assert!(entry.data.starts_with(PNG_SIGNATURE), "The 256 px entry must be PNG-compressed");
            let image = eframe::icon_data::from_png_bytes(entry.data).expect("256 px entry must be a valid PNG");
            assert_eq!((image.width, image.height), (256, 256));
        } else {
            // Small entries stay BMP for compatibility; height counts the AND mask too.
            let header = &entry.data[..16];
            assert_eq!(
                u32::from_le_bytes(header[0..4].try_into().unwrap()),
                40,
                "{} px entry must be a BMP",
                entry.size
            );
            assert_eq!(i32::from_le_bytes(header[4..8].try_into().unwrap()), entry.size as i32);
            assert_eq!(i32::from_le_bytes(header[8..12].try_into().unwrap()), 2 * entry.size as i32);
        }
    }
    assert!(APP_ICO.len() < 200 * 1024, "app.ico must not store the 256 px entry uncompressed");
}

#[test]
fn windows_icon_matches_the_linux_256_px_icon() {
    let png = entries().into_iter().find(|entry| entry.size == 256).unwrap();
    assert_eq!(png.data, include_bytes!("../build/linux/256x256.png"));
}
