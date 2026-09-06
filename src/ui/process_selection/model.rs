use std::{cmp::Ordering, collections::HashSet};

use crate::ProcessInfo;

use super::{ProcessSortColumn, SortDirection, cmp_ascii_case_insensitive, contains_ignore_ascii_case};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProcessRowKey {
    Group(String),
    Process { pid: process_memory::Pid, start_time: u64 },
}

impl ProcessRowKey {
    pub fn process(process: &ProcessInfo) -> Self {
        Self::Process {
            pid: process.pid,
            start_time: process.start_time,
        }
    }
}

#[derive(Default)]
pub struct ProcessSelectionState {
    pub selected: Option<ProcessRowKey>,
    pub expanded: HashSet<String>,
    /// Explicit collapses override automatic expansion for this filter only.
    pub collapsed_matches: HashSet<String>,
    pub last_filter: String,
    pub scroll_to_selection: bool,
    pub row_focus: Option<egui::Id>,
    pub message: Option<String>,
}

pub(super) struct ProcessRow<'a> {
    pub key: ProcessRowKey,
    pub process: &'a ProcessInfo,
    pub group_size: usize,
    pub matching_children: usize,
    pub expanded: bool,
    pub parent: Option<String>,
}

impl ProcessRow<'_> {
    pub fn is_group(&self) -> bool {
        self.group_size > 1
    }
}

pub(super) fn matches_filter(process: &ProcessInfo, filter: &str) -> bool {
    contains_ignore_ascii_case(&process.name, filter) || contains_ignore_ascii_case(&process.cmd, filter) || process.pid.to_string().contains(filter)
}

fn compare(a: &ProcessInfo, b: &ProcessInfo, column: ProcessSortColumn, direction: SortDirection) -> Ordering {
    let order = match column {
        ProcessSortColumn::Pid => a.pid.cmp(&b.pid),
        ProcessSortColumn::Name => cmp_ascii_case_insensitive(&a.name, &b.name),
        ProcessSortColumn::Memory => a.memory.cmp(&b.memory),
        ProcessSortColumn::Command => cmp_ascii_case_insensitive(&a.cmd, &b.cmd),
    };
    let order = if direction == SortDirection::Descending { order.reverse() } else { order };
    order.then_with(|| a.pid.cmp(&b.pid)).then_with(|| a.start_time.cmp(&b.start_time))
}

/// Filter actual instances, not representative/group metadata. A matching
/// child is always reachable, even when the group was previously collapsed.
pub(super) fn build_rows<'a>(
    processes: &'a [ProcessInfo],
    filter: &str,
    column: ProcessSortColumn,
    direction: SortDirection,
    state: &ProcessSelectionState,
) -> Vec<ProcessRow<'a>> {
    let mut top: Vec<_> = processes
        .iter()
        .filter(|process| {
            if process.instances.len() > 1 {
                process.instances.iter().any(|child| matches_filter(child, filter))
            } else {
                matches_filter(process, filter)
            }
        })
        .collect();
    top.sort_by(|a, b| compare(a, b, column, direction));
    let mut rows = Vec::new();
    for process in top {
        let group_size = process.instances.len();
        let is_group = group_size > 1;
        let mut children: Vec<_> = process.instances.iter().filter(|child| matches_filter(child, filter)).collect();
        let expanded = is_group
            && if filter.is_empty() {
                state.expanded.contains(&process.executable)
            } else {
                !state.collapsed_matches.contains(&process.executable)
            };
        rows.push(ProcessRow {
            key: if is_group {
                ProcessRowKey::Group(process.executable.clone())
            } else {
                ProcessRowKey::process(process)
            },
            process,
            group_size,
            matching_children: children.len(),
            expanded,
            parent: None,
        });
        if expanded {
            children.sort_by(|a, b| compare(a, b, column, direction));
            for child in children {
                rows.push(ProcessRow {
                    key: ProcessRowKey::process(child),
                    process: child,
                    group_size: 0,
                    matching_children: 0,
                    expanded: false,
                    parent: Some(process.executable.clone()),
                });
            }
        }
    }
    rows
}

pub(super) fn process_counts(processes: &[ProcessInfo], filter: &str) -> (usize, usize, usize) {
    let mut total = 0;
    let mut matched = 0;
    let mut groups = 0;
    for process in processes {
        if process.instances.len() > 1 {
            groups += 1;
            total += process.instances.len();
            matched += process.instances.iter().filter(|child| matches_filter(child, filter)).count();
        } else {
            total += 1;
            matched += usize::from(matches_filter(process, filter));
        }
    }
    (total, matched, groups)
}

impl ProcessSelectionState {
    pub(super) fn update_filter(&mut self, filter: &str) {
        if self.last_filter != filter {
            self.last_filter = filter.to_owned();
            self.collapsed_matches.clear();
            self.message = None;
        }
    }

    pub(super) fn toggle_group(&mut self, key: &str, expanded: bool, filtered: bool) {
        if expanded {
            self.expanded.remove(key);
            if filtered {
                self.collapsed_matches.insert(key.to_owned());
            }
        } else {
            self.expanded.insert(key.to_owned());
            self.collapsed_matches.remove(key);
        }
    }

    pub(super) fn reconcile(&mut self, rows: &[ProcessRow<'_>]) {
        if self.selected.as_ref().is_some_and(|key| !rows.iter().any(|row| &row.key == key)) {
            self.selected = None;
        }
    }

    pub(super) fn move_selection(&mut self, rows: &[ProcessRow<'_>], backwards: bool) {
        if rows.is_empty() {
            return;
        }
        let current = self.selected.as_ref().and_then(|key| rows.iter().position(|row| &row.key == key));
        let next = match current {
            Some(i) if backwards => i.saturating_sub(1),
            Some(i) => (i + 1).min(rows.len() - 1),
            None if backwards => rows.len() - 1,
            None => 0,
        };
        self.selected = Some(rows[next].key.clone());
        self.scroll_to_selection = true;
        self.message = None;
    }
}

pub(super) fn resolve_process<'a>(processes: &'a [ProcessInfo], key: &ProcessRowKey) -> Option<&'a ProcessInfo> {
    processes
        .iter()
        .flat_map(|process| {
            if process.instances.len() > 1 {
                process.instances.as_slice()
            } else {
                std::slice::from_ref(process)
            }
        })
        .find(|process| ProcessRowKey::process(process) == *key)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn process(pid: process_memory::Pid, name: &str, memory: usize) -> ProcessInfo {
        ProcessInfo {
            pid,
            start_time: 100,
            executable: format!("/games/{name}"),
            name: name.to_owned(),
            cmd: format!("/games/{name} --instance={pid}"),
            user: "test".to_owned(),
            memory,
            instances: Vec::new(),
        }
    }

    fn group() -> ProcessInfo {
        let a = process(10, "game", 10);
        let b = process(20, "game", 20);
        ProcessInfo {
            memory: 30,
            instances: vec![a.clone(), b],
            ..a
        }
    }

    #[test]
    fn pid_filter_expands_only_matching_child_and_counts_instances() {
        let processes = vec![group(), process(30, "other", 5)];
        let state = ProcessSelectionState::default();
        let rows = build_rows(&processes, "20", ProcessSortColumn::Memory, SortDirection::Descending, &state);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_group());
        assert!(rows[0].expanded);
        assert_eq!(rows[0].matching_children, 1);
        assert_eq!(rows[1].process.pid, 20);
        assert_eq!(process_counts(&processes, ""), (3, 3, 1));
        assert_eq!(process_counts(&processes, "20"), (3, 1, 1));
        assert_eq!(process_counts(&processes, "/GAMES/GAME"), (3, 2, 1));
    }

    #[test]
    fn collapsed_filter_matches_can_be_reopened() {
        let processes = vec![group()];
        let mut state = ProcessSelectionState::default();
        state.update_filter("20");
        state.toggle_group("/games/game", true, true);
        assert_eq!(build_rows(&processes, "20", ProcessSortColumn::Pid, SortDirection::Ascending, &state).len(), 1);
        state.toggle_group("/games/game", false, true);
        assert_eq!(build_rows(&processes, "20", ProcessSortColumn::Pid, SortDirection::Ascending, &state).len(), 2);
        state.toggle_group("/games/game", true, true);
        state.update_filter("game");
        assert_eq!(
            build_rows(&processes, "game", ProcessSortColumn::Pid, SortDirection::Ascending, &state).len(),
            3
        );
    }

    #[test]
    fn stable_sort_preserves_identity_and_rejects_recycled_pid() {
        let mut processes = vec![process(20, "game", 10), process(10, "game", 10)];
        let key = ProcessRowKey::process(&processes[0]);
        let mut state = ProcessSelectionState {
            selected: Some(key.clone()),
            ..Default::default()
        };
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            let rows = build_rows(&processes, "", ProcessSortColumn::Memory, direction, &state);
            assert_eq!(rows[0].process.pid, 10);
            state.reconcile(&rows);
            assert_eq!(state.selected, Some(key.clone()));
        }
        processes[0].memory = 100;
        let rows = build_rows(&processes, "", ProcessSortColumn::Memory, SortDirection::Descending, &state);
        state.reconcile(&rows);
        assert_eq!(state.selected, Some(key.clone()));
        processes[0].start_time += 1;
        assert!(resolve_process(&processes, &key).is_none());
        let rows = build_rows(&processes, "", ProcessSortColumn::Memory, SortDirection::Descending, &state);
        state.reconcile(&rows);
        assert_eq!(state.selected, None);
    }

    #[test]
    fn groups_keep_expansion_when_representative_changes_and_children_follow_sort() {
        let mut processes = vec![group()];
        let mut state = ProcessSelectionState::default();
        state.expanded.insert("/games/game".to_owned());
        processes[0].pid = 99;
        let rows = build_rows(&processes, "", ProcessSortColumn::Memory, SortDirection::Descending, &state);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].process.pid, 20);
        assert!(resolve_process(&processes, &rows[0].key).is_none());
        assert_eq!(resolve_process(&processes, &rows[1].key).unwrap().pid, 20);
    }

    #[test]
    fn keyboard_selection_clamps_and_filtered_selection_is_cleared() {
        let processes = vec![process(1, "alpha", 10), process(2, "beta", 20)];
        let mut state = ProcessSelectionState::default();
        let rows = build_rows(&processes, "", ProcessSortColumn::Name, SortDirection::Ascending, &state);
        state.move_selection(&rows, false);
        assert_eq!(state.selected, Some(rows[0].key.clone()));
        state.move_selection(&rows, false);
        state.move_selection(&rows, false);
        assert_eq!(state.selected, Some(rows[1].key.clone()));
        let rows = build_rows(&processes, "alpha", ProcessSortColumn::Name, SortDirection::Ascending, &state);
        state.reconcile(&rows);
        assert_eq!(state.selected, None);
    }
}
