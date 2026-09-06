use super::*;

fn frame(editor: &mut MemoryEditor, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 400.0))),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                inspector_body(editor, ui);
            });
        },
    );
    output.textures_delta.clear();
    output
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn text_rect(output: &egui::FullOutput, label: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => Some(text.galley.rect.translate(text.pos.to_vec2())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing inspector label {label}"))
}

fn edit_field(editor: &mut MemoryEditor, ctx: &egui::Context, kind: InspectorKind, text: &str) {
    frame(editor, ctx, vec![]);
    let output = frame(editor, ctx, vec![]);
    let label = text_rect(&output, kind.label());
    let pos = egui::pos2(label.right() + 40.0, label.center().y);
    frame(editor, ctx, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            editor,
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    assert!(ctx.memory(|memory| memory.focused().is_some()), "Click must focus the inspector field");
    frame(
        editor,
        ctx,
        vec![key(egui::Key::A, egui::Modifiers::COMMAND), egui::Event::Text(text.to_owned())],
    );
    let edit = editor.data.inspector_edit.as_ref().expect("Typing must start an inspector edit");
    assert_eq!(edit.kind, kind);
    assert_eq!(edit.text, text);
}

#[cfg(target_os = "linux")]
#[test]
fn inspector_enter_writes_u32_and_records_one_reversible_write() {
    let mut bytes = Box::new([0x39, 0x30, 0, 0, 0xAA, 0xBB, 0xCC, 0xDD]);
    let addr = bytes.as_mut_ptr() as usize;
    let mut editor = MemoryEditor::default();
    editor.initialize(std::process::id() as _, addr, SearchType::Int, 4).unwrap();
    // The full memory-editor view attaches this handle before rendering the
    // inspector; this isolated panel test must do the same.
    editor.data.handle = Some((std::process::id() as process_memory::Pid).try_into_process_handle().unwrap());
    let ctx = egui::Context::default();
    crate::ui::theme::apply(&ctx);
    edit_field(&mut editor, &ctx, InspectorKind::U32, "54321");
    assert_eq!(&bytes[..4], &12345_u32.to_le_bytes(), "Typing must not write before Enter");
    frame(&mut editor, &ctx, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    assert_eq!(&bytes[..4], &54321_u32.to_le_bytes(), "Enter must write the edited u32");
    assert_eq!(&bytes[4..], &[0xAA, 0xBB, 0xCC, 0xDD]);
    assert!(editor.data.inspector_edit.is_none());
    assert_eq!(editor.data.undo_stack.len(), 1);
    assert!(editor.undo());
    assert_eq!(&bytes[..4], &12345_u32.to_le_bytes());
    assert!(editor.redo());
    assert_eq!(&bytes[..4], &54321_u32.to_le_bytes());
}

fn cached_editor() -> MemoryEditor {
    let mut editor = MemoryEditor::default();
    editor.raw.set_address_range("test", 0x1000..0x1008);
    editor.raw.goto_address(0x1000);
    editor.data.current_result_type = Some(SearchType::Int);
    for (offset, byte) in 12345_u64.to_le_bytes().into_iter().enumerate() {
        editor.data.cache.insert(0x1000 + offset, Some(byte));
    }
    editor
}

#[test]
fn inspector_escape_discards_input_without_writing() {
    let mut editor = cached_editor();
    let ctx = egui::Context::default();
    edit_field(&mut editor, &ctx, InspectorKind::U32, "54321");
    frame(&mut editor, &ctx, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    assert!(editor.data.inspector_edit.is_none());
    assert!(editor.data.inspector_error.is_none());
    assert!(editor.data.undo_stack.is_empty());
    assert_eq!(read_current(&editor.data, 0x1000, 4), 12345_u32.to_le_bytes());
}

#[test]
fn inspector_failed_write_preserves_input_focus_and_shows_error() {
    let mut editor = cached_editor();
    let ctx = egui::Context::default();
    edit_field(&mut editor, &ctx, InspectorKind::U32, "54321");
    let focus = ctx.memory(|memory| memory.focused());
    // No handle: the write must fail without touching any real memory.
    frame(&mut editor, &ctx, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    let output = frame(&mut editor, &ctx, vec![]);
    assert_eq!(editor.data.inspector_edit.as_ref().unwrap().text, "54321");
    assert_eq!(ctx.memory(|memory| memory.focused()), focus);
    assert!(editor.data.undo_stack.is_empty());
    assert_eq!(read_current(&editor.data, 0x1000, 4), 12345_u32.to_le_bytes());
    let error = editor.data.inspector_error.as_ref().expect("Write failure must be reported");
    text_rect(&output, error);
    frame(&mut editor, &ctx, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    assert!(editor.data.inspector_edit.is_none());
    assert!(editor.data.inspector_error.is_none());
}

#[test]
fn inspector_invalid_enter_preserves_input_and_reports_parse_error() {
    let mut editor = cached_editor();
    let ctx = egui::Context::default();
    edit_field(&mut editor, &ctx, InspectorKind::U32, "not-a-number");
    let focus = ctx.memory(|memory| memory.focused());
    frame(&mut editor, &ctx, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    assert_eq!(editor.data.inspector_edit.as_ref().unwrap().text, "not-a-number");
    assert!(editor.data.inspector_error.is_some());
    assert_eq!(ctx.memory(|memory| memory.focused()), focus);
    assert!(editor.data.undo_stack.is_empty());
}

#[test]
fn inspector_header_uses_localized_messages() {
    let mut editor = cached_editor();
    let ctx = egui::Context::default();
    frame(&mut editor, &ctx, vec![]);
    let output = frame(&mut editor, &ctx, vec![]);
    for label in [
        fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-title"),
        fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-edit-hint"),
        fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-type-label"),
        fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-unsigned"),
        fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-signed"),
        fl!(crate::LANGUAGE_LOADER, "memory-editor-inspector-float-raw"),
    ] {
        text_rect(&output, &label);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn inspector_enter_respects_numeric_kind_and_byte_order() {
    for (kind, text, little_bytes) in [
        (InspectorKind::I16, "-1234", (-1234_i16).to_le_bytes().to_vec()),
        (InspectorKind::U32, "305419896", 0x12345678_u32.to_le_bytes().to_vec()),
        (InspectorKind::I64, "-1234567890123", (-1234567890123_i64).to_le_bytes().to_vec()),
        (InspectorKind::F32, "1.5", 1.5_f32.to_le_bytes().to_vec()),
        (InspectorKind::F64, "-2.25", (-2.25_f64).to_le_bytes().to_vec()),
    ] {
        for endian in [Endianness::Little, Endianness::Big] {
            let mut bytes = Box::new([0_u8; 16]);
            let addr = bytes.as_mut_ptr() as usize;
            let mut editor = MemoryEditor::default();
            editor.initialize(std::process::id() as _, addr, SearchType::Int, 4).unwrap();
            editor.data.handle = Some((std::process::id() as process_memory::Pid).try_into_process_handle().unwrap());
            editor.raw.set_endianness(endian);
            let ctx = egui::Context::default();
            edit_field(&mut editor, &ctx, kind, text);
            assert_eq!(*bytes, [0; 16], "No writes before Enter");
            frame(&mut editor, &ctx, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
            let mut expected = little_bytes.clone();
            if matches!(endian, Endianness::Big) {
                expected.reverse();
            }
            assert_eq!(&bytes[..expected.len()], &expected, "Wrong write for {kind:?} / {endian:?}");
            assert!(bytes[expected.len()..].iter().all(|&b| b == 0), "Adjacent bytes must not change");
            assert_eq!(editor.data.undo_stack.len(), 1);
        }
    }
}
