//! Address navigation, independent of process attachment and write undo/redo.

use super::{Address, MemoryEditor};

const HISTORY_LIMIT: usize = 128;

#[derive(Default)]
pub(super) struct NavigationHistory {
    back: Vec<Address>,
    forward: Vec<Address>,
}

fn push_bounded(stack: &mut Vec<Address>, address: Address) {
    if stack.len() == HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(address);
}

impl MemoryEditor {
    pub(super) fn current_address(&self) -> Address {
        self.raw.highlighted_address().unwrap_or(self.data.origin_address)
    }

    fn is_navigation_target(&self, address: Address) -> bool {
        self.data.regions.iter().any(|region| region.readable && region.range.contains(&address))
    }

    /// Recenter even on a repeated jump, without retaining an old write caret.
    fn show_navigation_address(&mut self, address: Address) {
        self.raw.goto_address(address);
        self.raw.set_caret(None);
        self.data.inspector_edit = None;
        self.data.inspector_error = None;
    }

    pub(super) fn navigate_to(&mut self, address: Address) -> bool {
        if !self.is_navigation_target(address) {
            return false;
        }

        let current = self.current_address();
        if current != address {
            push_bounded(&mut self.data.navigation.back, current);
            self.data.navigation.forward.clear();
        }
        self.show_navigation_address(address);
        true
    }

    pub(super) fn can_go_back(&self) -> bool {
        self.data.navigation.back.last().is_some_and(|&address| self.is_navigation_target(address))
    }

    pub(super) fn can_go_forward(&self) -> bool {
        self.data.navigation.forward.last().is_some_and(|&address| self.is_navigation_target(address))
    }

    pub(super) fn go_back(&mut self) -> bool {
        let Some(address) = self.data.navigation.back.last().copied() else {
            return false;
        };
        if !self.is_navigation_target(address) {
            return false;
        }

        let current = self.current_address();
        self.data.navigation.back.pop();
        push_bounded(&mut self.data.navigation.forward, current);
        self.show_navigation_address(address);
        true
    }

    pub(super) fn go_forward(&mut self) -> bool {
        let Some(address) = self.data.navigation.forward.last().copied() else {
            return false;
        };
        if !self.is_navigation_target(address) {
            return false;
        }

        let current = self.current_address();
        self.data.navigation.forward.pop();
        push_bounded(&mut self.data.navigation.back, current);
        self.show_navigation_address(address);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchType;
    use crate::ui::mem_editor::{InspectorEdit, InspectorKind, RegionInfo, WriteRecord};
    use std::time::Instant;

    const A: Address = 0x1000;
    const B: Address = 0x2000;
    const C: Address = 0x3000;

    fn editor() -> MemoryEditor {
        let mut editor = MemoryEditor::default();
        for (name, address) in [("A", A), ("B", B), ("C", C)] {
            let range = address..address + 0x100;
            editor.raw.set_address_range(name, range.clone());
            editor.data.regions.push(RegionInfo {
                range,
                label: name.into(),
                name: name.into(),
                readable: true,
                writable: false,
            });
        }
        editor.data.origin_address = A;
        editor.raw.goto_address(A);
        editor
    }

    fn history(editor: &MemoryEditor) -> (Vec<Address>, Vec<Address>) {
        (editor.data.navigation.back.clone(), editor.data.navigation.forward.clone())
    }

    fn dirty_inspector(editor: &mut MemoryEditor) {
        let address = editor.current_address();
        editor.raw.set_caret(Some((address, true)));
        editor.data.inspector_edit = Some(InspectorEdit {
            address,
            kind: InspectorKind::U32,
            text: "unfinished".into(),
        });
        editor.data.inspector_error = Some("write failed".into());
    }

    fn assert_clean(editor: &MemoryEditor) {
        assert!(editor.raw.caret().is_none());
        assert!(editor.data.inspector_edit.is_none());
        assert!(editor.data.inspector_error.is_none());
        assert!(editor.data.handle.is_none());
        assert_eq!(editor.data.origin_address, A);
    }

    #[test]
    fn current_address_falls_back_to_origin() {
        let mut editor = MemoryEditor::default();
        editor.data.origin_address = A;
        assert_eq!(editor.current_address(), A);
        assert!(!editor.can_go_back());
        assert!(!editor.can_go_forward());
        assert!(!editor.go_back());
        assert!(!editor.go_forward());
    }

    #[test]
    fn pointer_like_jumps_round_trip_across_ranges() {
        let mut editor = editor();
        assert!(editor.navigate_to(B));
        assert_eq!(editor.raw.selected_range_name(), "B");
        assert!(editor.navigate_to(C));
        assert_eq!(editor.raw.selected_range_name(), "C");
        assert_eq!(history(&editor), (vec![A, B], vec![]));
        assert!(editor.can_go_back());
        assert!(!editor.can_go_forward());
        assert!(editor.go_back());
        assert_eq!(editor.current_address(), B);
        assert_eq!(editor.raw.selected_range_name(), "B");
        assert!(editor.go_back());
        assert_eq!(editor.current_address(), A);
        assert_eq!(editor.raw.selected_range_name(), "A");
        assert!(!editor.go_back());
        assert!(editor.can_go_forward());
        assert!(editor.go_forward());
        assert_eq!(editor.current_address(), B);
        assert!(editor.go_forward());
        assert_eq!(editor.current_address(), C);
        assert!(!editor.go_forward());
        assert_eq!(history(&editor), (vec![A, B], vec![]));
        assert_clean(&editor);
    }

    #[test]
    fn same_address_recenters_without_duplicates_or_losing_forward() {
        let mut editor = editor();
        assert!(editor.navigate_to(A));
        assert_eq!(history(&editor), (vec![], vec![]));
        assert!(editor.navigate_to(B));
        assert!(editor.navigate_to(C));
        assert!(editor.go_back());
        let before = history(&editor);
        dirty_inspector(&mut editor);
        // Moving only the selected range leaves the highlight at B. A repeated
        // jump must still call raw.goto_address to restore B's range/centering.
        editor.raw.select_range("A");
        assert!(editor.navigate_to(B));
        assert_eq!(editor.raw.selected_range_name(), "B");
        assert_eq!(history(&editor), before);
        assert_clean(&editor);
        assert!(editor.go_forward());
        assert_eq!(editor.current_address(), C);
    }

    #[test]
    fn manual_highlight_is_the_actual_source_in_each_direction() {
        let mut editor = editor();
        editor.raw.goto_address(A + 7);
        assert!(editor.navigate_to(B));
        assert_eq!(editor.data.navigation.back, vec![A + 7]);
        editor.raw.goto_address(B + 9);
        assert!(editor.go_back());
        assert_eq!(editor.current_address(), A + 7);
        assert_eq!(editor.data.navigation.forward, vec![B + 9]);
        editor.raw.goto_address(A + 11);
        assert!(editor.go_forward());
        assert_eq!(editor.current_address(), B + 9);
        assert!(editor.go_back());
        assert_eq!(editor.current_address(), A + 11);
        assert_eq!(editor.data.origin_address, A);
    }

    #[test]
    fn invalid_or_unreadable_jumps_preserve_history_and_edit_state() {
        let mut editor = editor();
        assert!(editor.navigate_to(B));
        assert!(editor.go_back());
        editor.data.regions[2].readable = false;
        dirty_inspector(&mut editor);
        let before = history(&editor);
        for address in [0, A - 1, A + 0x100, C, Address::MAX] {
            assert!(!editor.navigate_to(address));
            assert_eq!(history(&editor), before);
            assert_eq!(editor.current_address(), A);
            assert_eq!(editor.raw.caret(), Some((A, true)));
            assert_eq!(editor.data.inspector_edit.as_ref().unwrap().text, "unfinished");
            assert_eq!(editor.data.inspector_error.as_deref(), Some("write failed"));
        }
    }

    #[test]
    fn back_and_forward_validate_before_mutating_either_stack() {
        let mut editor = editor();
        assert!(editor.navigate_to(B));
        assert!(editor.navigate_to(C));
        editor.data.regions[1].readable = false;
        let before = history(&editor);
        assert!(!editor.can_go_back());
        assert!(!editor.go_back());
        assert_eq!(editor.current_address(), C);
        assert_eq!(history(&editor), before);
        editor.data.regions[1].readable = true;
        assert!(editor.go_back());
        let before = history(&editor);
        editor.data.regions[2].readable = false;
        assert!(!editor.can_go_forward());
        assert!(!editor.go_forward());
        assert_eq!(editor.current_address(), B);
        assert_eq!(history(&editor), before);
        editor.data.regions.clear();
        assert!(!editor.go_back());
        assert!(!editor.go_forward());
        assert_eq!(history(&editor), before);
    }

    #[test]
    fn a_real_branch_discards_forward_history() {
        let mut editor = editor();
        assert!(editor.navigate_to(B));
        assert!(editor.navigate_to(C));
        assert!(editor.go_back());
        assert!(editor.navigate_to(A + 1));
        assert_eq!(history(&editor), (vec![A, B], vec![]));
        assert!(!editor.can_go_forward());
        assert!(!editor.go_forward());
        assert!(editor.go_back());
        assert_eq!(editor.current_address(), B);
    }

    #[test]
    fn history_is_bounded_to_128_entries_in_both_directions() {
        let mut editor = editor();
        for offset in 1..=200 {
            assert!(editor.navigate_to(A + offset));
            assert!(editor.data.navigation.back.len() <= HISTORY_LIMIT);
        }
        assert_eq!(editor.data.navigation.back, (A + 72..A + 200).collect::<Vec<_>>());
        for _ in 0..HISTORY_LIMIT {
            assert!(editor.go_back());
            assert!(editor.data.navigation.forward.len() <= HISTORY_LIMIT);
        }
        assert_eq!(editor.current_address(), A + 72);
        assert!(!editor.go_back());
        for _ in 0..HISTORY_LIMIT {
            assert!(editor.go_forward());
            assert!(editor.data.navigation.back.len() <= HISTORY_LIMIT);
        }
        assert_eq!(editor.current_address(), A + 200);
        assert!(!editor.go_forward());

        // Exercise overflow of the forward stack as well as the back stack.
        editor.data.navigation.forward = (A..A + HISTORY_LIMIT).collect();
        assert!(editor.go_back());
        assert_eq!(editor.data.navigation.forward.len(), HISTORY_LIMIT);
        assert_eq!(editor.data.navigation.forward.first(), Some(&(A + 1)));
        assert_eq!(editor.data.navigation.forward.last(), Some(&(A + 200)));
    }

    #[test]
    fn successful_navigation_clears_dirty_editor_state_but_preserves_data() {
        let mut editor = editor();
        let record = WriteRecord {
            addr: A,
            before: vec![1],
            after: vec![2],
            caret_before: Some((A, false)),
            caret_after: Some((A, true)),
        };
        editor.data.undo_stack.push(record.clone());
        editor.data.redo_stack.push(record);
        editor.data.cache.insert(A, Some(2));
        let changed_at = Instant::now();
        editor.data.change_tracker.insert(A, (2, changed_at));
        editor.data.current_result_type = Some(SearchType::Int);
        editor.data.current_result_byte_length = 4;
        editor.raw.set_result_highlight_range(Some(A..A + 4));

        dirty_inspector(&mut editor);
        assert!(editor.navigate_to(B));
        assert_clean(&editor);
        dirty_inspector(&mut editor);
        assert!(editor.go_back());
        assert_clean(&editor);
        dirty_inspector(&mut editor);
        assert!(editor.go_forward());
        assert_clean(&editor);

        assert_eq!(editor.data.undo_stack.len(), 1);
        assert_eq!(editor.data.undo_stack[0].before, vec![1]);
        assert_eq!(editor.data.redo_stack.len(), 1);
        assert_eq!(editor.data.redo_stack[0].after, vec![2]);
        assert_eq!(editor.data.cache.get(&A), Some(&Some(2)));
        assert_eq!(editor.data.change_tracker.get(&A), Some(&(2, changed_at)));
        assert_eq!(editor.data.current_result_type, Some(SearchType::Int));
        assert_eq!(editor.data.current_result_byte_length, 4);
    }
}
