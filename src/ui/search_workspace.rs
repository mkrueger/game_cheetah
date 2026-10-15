use std::collections::HashMap;

use egui::{Color32, CornerRadius, Rect, Stroke, StrokeKind, pos2, vec2};
use egui_tiles::{Behavior, Container, ContainerKind, LinearDir, Tile, TileId, Tiles, Tree, UiResponse};
use i18n_embed_fl::fl;

use super::{app::App, in_process_view, value_cache::ValueCache};
use crate::SearchResult;

/// Gutter between cards; dark enough that each tab group reads as one unit.
pub(crate) const WORKSPACE_BG: Color32 = Color32::from_rgb(13, 15, 18);
const STRIP_BG: Color32 = Color32::from_rgb(23, 26, 31);
const CONTENT_BG: Color32 = Color32::from_rgb(18, 20, 23);
const CARD_BORDER: Color32 = Color32::from_rgb(44, 49, 57);
const CARD_RADIUS: u8 = 8;
const TAB_BAR_HEIGHT: f32 = 36.0;
const TAB_TOP_MARGIN: f32 = 6.0;
const TAB_PADDING: f32 = 12.0;
const TAB_GAP: f32 = 2.0;
const TAB_RADIUS: u8 = 6;
const CLOSE_SIZE: f32 = 16.0;
const MUTED_TEXT: Color32 = Color32::from_rgb(150, 156, 166);
const BRIGHT_TEXT: Color32 = Color32::from_rgb(232, 235, 240);

pub(crate) struct SearchWorkspace {
    tree: Tree<u64>,
    inactive: HashMap<u64, InactiveSearchView>,
    last_active: Option<u64>,
    /// Requested from the top bar, applied before the next tree pass.
    pending_layout: Option<ContainerKind>,
}

impl Default for SearchWorkspace {
    fn default() -> Self {
        Self {
            tree: Tree::empty("search_workspace"),
            inactive: HashMap::new(),
            last_active: None,
            pending_layout: None,
        }
    }
}

impl SearchWorkspace {
    pub(crate) fn clear_cached_values(&mut self) {
        self.inactive.clear();
    }

    fn sync(&mut self, app: &App) {
        let ids: Vec<_> = app.state.searches.iter().map(|search| search.view_id).collect();
        let removed: Vec<_> = self
            .tree
            .tiles
            .iter()
            .filter_map(|(&tile_id, tile)| matches!(tile, Tile::Pane(id) if !ids.contains(id)).then_some(tile_id))
            .collect();
        for tile_id in removed {
            self.tree.remove_recursively(tile_id);
        }
        self.inactive.retain(|id, _| ids.contains(id));
        self.tree.simplify(&simplification_options());

        let parent = self
            .last_active
            .and_then(|id| self.tree.tiles.find_pane(&id))
            .and_then(|tile| self.tree.tiles.parent_of(tile));
        for id in &ids {
            if self.tree.tiles.find_pane(id).is_some() {
                continue;
            }
            let pane = self.tree.tiles.insert_pane(*id);
            if let Some(Tile::Container(Container::Tabs(tabs))) = parent.and_then(|parent| self.tree.tiles.get_mut(parent)) {
                tabs.children.push(pane);
            } else if let Some(Tile::Container(Container::Tabs(tabs))) = self.tree.root.and_then(|root| self.tree.tiles.get_mut(root)) {
                tabs.children.push(pane);
            } else {
                let children = self.tree.root.into_iter().chain(std::iter::once(pane)).collect();
                self.tree.root = Some(self.tree.tiles.insert_tab_tile(children));
            }
        }
        let active = ids[app.state.current_search];
        if self.last_active != Some(active) {
            self.tree.make_active(|_, tile| matches!(tile, Tile::Pane(id) if *id == active));
            self.last_active = Some(active);
        }
    }

    fn arrange(&mut self, kind: ContainerKind) {
        // Reuse leaf tile IDs so changing layouts does not reset widget state.
        let panes: Vec<_> = self
            .tree
            .tiles
            .iter()
            .filter_map(|(&tile, content)| match content {
                Tile::Pane(id) => Some((*id, tile)),
                Tile::Container(_) => None,
            })
            .collect();
        let mut panes = panes;
        panes.sort_by_key(|(id, _)| *id);
        let containers: Vec<_> = self
            .tree
            .tiles
            .iter()
            .filter_map(|(&tile, content)| matches!(content, Tile::Container(_)).then_some(tile))
            .collect();
        for tile in containers {
            self.tree.tiles.remove(tile);
        }
        self.tree.root = Some(
            self.tree
                .tiles
                .insert_container(Container::new(kind, panes.into_iter().map(|(_, tile)| tile).collect())),
        );
        if let Some(active) = self.last_active {
            self.tree.make_active(|_, tile| matches!(tile, Tile::Pane(id) if *id == active));
        }
    }

    /// The preset matching the current tree, or `None` after manual docking.
    fn current_layout(&self) -> Option<ContainerKind> {
        let root = self.tree.root?;
        let Tile::Container(container) = self.tree.tiles.get(root)? else {
            return None;
        };
        let single_pane_group = |child: &TileId| {
            matches!(self.tree.tiles.get(*child), Some(Tile::Container(Container::Tabs(tabs)))
                if tabs.children.len() == 1 && self.tree.tiles.get_pane(&tabs.children[0]).is_some())
        };
        match container {
            Container::Tabs(tabs) => tabs
                .children
                .iter()
                .all(|child| self.tree.tiles.get_pane(child).is_some())
                .then_some(ContainerKind::Tabs),
            Container::Linear(linear) => linear.children.iter().all(single_pane_group).then_some(match linear.dir {
                LinearDir::Horizontal => ContainerKind::Horizontal,
                LinearDir::Vertical => ContainerKind::Vertical,
            }),
            Container::Grid(grid) => grid.children().all(single_pane_group).then_some(ContainerKind::Grid),
        }
    }

    pub(crate) fn visible_searches(&self) -> Vec<u64> {
        self.tree
            .active_tiles()
            .into_iter()
            .filter_map(|tile| self.tree.tiles.get_pane(&tile).copied())
            .collect()
    }

    pub(crate) fn has_live_values(&self, state: &crate::GameCheetahEngine) -> bool {
        let visible = self.visible_searches();
        state.searches.iter().enumerate().any(|(index, search)| {
            let active = index == state.current_search;
            let count = search.get_result_count();
            let expanded = if active { state.show_results } else { search.show_results };
            (active || visible.contains(&search.view_id))
                && search.searching == crate::SearchMode::None
                && count > 0
                && (count <= in_process_view::BROWSE_RESULT_LIMIT || expanded)
        })
    }
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let mut workspace = std::mem::take(&mut app.search_workspace);
    workspace.sync(app);
    if let Some(kind) = workspace.pending_layout.take() {
        workspace.arrange(kind);
    }
    if app.renaming_search_index.is_none()
        && app.selected_result.is_none()
        && ui.memory(|memory| memory.focused().is_none())
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F2))
    {
        app.begin_rename_search(app.state.current_search);
    }
    let focused = app.state.searches[app.state.current_search].view_id;
    let groups = workspace
        .tree
        .tiles
        .iter()
        .filter_map(|(&tile, content)| match content {
            Tile::Container(Container::Tabs(tabs)) => {
                let shows_focused = tabs.active.and_then(|active| workspace.tree.tiles.get_pane(&active)) == Some(&focused);
                Some((tile, shows_focused))
            }
            _ => None,
        })
        .collect();
    let highlight_focus = workspace.visible_searches().len() > 1;
    let mut behavior = SearchBehavior {
        app,
        inactive: &mut workspace.inactive,
        action: None,
        groups,
        highlight_focus,
    };
    workspace.tree.ui(&mut behavior, ui);
    if let Some(action) = behavior.action {
        match action {
            TabAction::Select(id) => {
                if let Some(index) = search_index(app, id) {
                    app.switch_search(index);
                }
            }
            TabAction::Rename(id) => {
                if let Some(index) = search_index(app, id) {
                    app.begin_rename_search(index);
                }
            }
            TabAction::Close(id) => {
                if let Some(index) = search_index(app, id) {
                    app.close_search(index);
                }
            }
            TabAction::CloseOthers(id) => {
                if let Some(index) = search_index(app, id) {
                    app.close_other_searches(index);
                }
            }
            TabAction::New(parent) => {
                app.new_search();
                let pane = workspace.tree.tiles.insert_pane(app.state.searches[app.state.current_search].view_id);
                if let Some(Tile::Container(Container::Tabs(tabs))) = workspace.tree.tiles.get_mut(parent) {
                    tabs.children.push(pane);
                    tabs.active = Some(pane);
                }
            }
        }
    }
    workspace.sync(app);
    let active = app.state.searches[app.state.current_search].view_id;
    let visible = workspace.visible_searches();
    if !visible.contains(&active)
        && let Some(index) = visible.first().and_then(|id| search_index(app, *id))
    {
        app.switch_search(index);
        workspace.last_active = Some(app.state.searches[index].view_id);
    }
    app.search_workspace = workspace;
}

/// Compact layout picker for the top bar; shows the current preset's icon.
pub(crate) fn layout_picker(app: &mut App, ui: &mut egui::Ui) {
    let current = app.search_workspace.current_layout();
    let label = fl!(crate::LANGUAGE_LOADER, "search-layout");
    let font = egui::FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(label, font, BRIGHT_TEXT);
    let size = vec2(10.0 + 18.0 + 8.0 + galley.size().x + 8.0 + 10.0 + 10.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    if response.hovered() || open {
        ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(36, 40, 46));
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 19.0, rect.center().y), vec2(18.0, 14.0));
    paint_layout_icon(ui.painter(), icon, current.unwrap_or(ContainerKind::Tabs), BRIGHT_TEXT);
    let text_pos = pos2(icon.right() + 8.0, rect.center().y - galley.size().y / 2.0);
    let text_width = galley.size().x;
    ui.painter().galley(text_pos, galley, BRIGHT_TEXT);
    paint_chevron(ui.painter(), pos2(text_pos.x + text_width + 13.0, rect.center().y), MUTED_TEXT);
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(fl!(crate::LANGUAGE_LOADER, "search-layout-hint"));
    egui::Popup::menu(&response).gap(4.0).show(|ui| {
        for (kind, label) in [
            (ContainerKind::Tabs, fl!(crate::LANGUAGE_LOADER, "search-layout-tabs")),
            (ContainerKind::Horizontal, fl!(crate::LANGUAGE_LOADER, "search-layout-horizontal")),
            (ContainerKind::Vertical, fl!(crate::LANGUAGE_LOADER, "search-layout-vertical")),
            (ContainerKind::Grid, fl!(crate::LANGUAGE_LOADER, "search-layout-grid")),
        ] {
            if layout_menu_item(ui, kind, label, current == Some(kind)).clicked() {
                app.search_workspace.pending_layout = Some(kind);
                ui.close();
            }
        }
    });
}

fn layout_menu_item(ui: &mut egui::Ui, kind: ContainerKind, label: String, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(240.0, 34.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(42, 47, 54));
    }
    let color = if selected { crate::ui::theme::ACCENT_TEXT } else { BRIGHT_TEXT };
    let icon = Rect::from_center_size(pos2(rect.left() + 19.0, rect.center().y), vec2(18.0, 14.0));
    paint_layout_icon(ui.painter(), icon, kind, color);
    ui.painter().text(
        pos2(icon.right() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label.clone(),
        egui::FontId::proportional(15.0),
        color,
    );
    if selected {
        let c = pos2(rect.right() - 16.0, rect.center().y);
        let stroke = Stroke::new(1.8, color);
        ui.painter().line_segment([c + vec2(-5.0, 0.0), c + vec2(-1.5, 3.5)], stroke);
        ui.painter().line_segment([c + vec2(-1.5, 3.5), c + vec2(5.0, -4.0)], stroke);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Small outline glyph showing how panes are arranged.
fn paint_layout_icon(painter: &egui::Painter, rect: Rect, kind: ContainerKind, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    painter.rect_stroke(rect, CornerRadius::same(2), stroke, StrokeKind::Inside);
    let c = rect.center();
    match kind {
        ContainerKind::Tabs => {
            let y = rect.top() + 4.5;
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
            painter.rect_filled(
                Rect::from_min_max(rect.min, pos2(rect.left() + rect.width() * 0.45, y)),
                CornerRadius::same(1),
                color,
            );
        }
        ContainerKind::Horizontal => {
            painter.line_segment([pos2(c.x, rect.top()), pos2(c.x, rect.bottom())], stroke);
        }
        ContainerKind::Vertical => {
            painter.line_segment([pos2(rect.left(), c.y), pos2(rect.right(), c.y)], stroke);
        }
        ContainerKind::Grid => {
            painter.line_segment([pos2(c.x, rect.top()), pos2(c.x, rect.bottom())], stroke);
            painter.line_segment([pos2(rect.left(), c.y), pos2(rect.right(), c.y)], stroke);
        }
    }
}

fn paint_chevron(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    painter.line_segment([center + vec2(-4.0, -2.0), center + vec2(0.0, 2.0)], stroke);
    painter.line_segment([center + vec2(0.0, 2.0), center + vec2(4.0, -2.0)], stroke);
}

fn compact_count(count: usize) -> String {
    match count {
        0..=9_999 => count.to_string(),
        10_000..=999_999 => format!("{}k", count / 1000),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

fn search_index(app: &App, id: u64) -> Option<usize> {
    app.state.searches.iter().position(|search| search.view_id == id)
}

enum TabAction {
    Select(u64),
    Rename(u64),
    Close(u64),
    CloseOthers(u64),
    New(TileId),
}

struct SearchBehavior<'a> {
    app: &'a mut App,
    inactive: &'a mut HashMap<u64, InactiveSearchView>,
    action: Option<TabAction>,
    /// Tab groups and whether each currently shows the focused search.
    groups: HashMap<TileId, bool>,
    /// The focus outline only helps when several searches are visible.
    highlight_focus: bool,
}

impl SearchBehavior<'_> {
    fn title(&self, tiles: &Tiles<u64>, tile_id: TileId) -> String {
        match tiles.get(tile_id) {
            Some(Tile::Pane(id)) => search_index(self.app, *id)
                .map(|index| self.app.state.searches[index].description.clone())
                .unwrap_or_default(),
            // A group dropped onto a tab bar becomes a nested tab: name its searches.
            Some(Tile::Container(container)) => container.children().map(|child| self.title(tiles, *child)).collect::<Vec<_>>().join(" · "),
            None => String::new(),
        }
    }

    fn rename_tab_ui(&mut self, ui: &mut egui::Ui, index: usize) -> egui::Response {
        let view_id = self.app.state.searches[index].view_id;
        let (outer, _) = ui.allocate_exact_size(vec2(170.0 + TAB_GAP * 2.0, TAB_BAR_HEIGHT), egui::Sense::hover());
        let rect = Rect::from_min_max(
            pos2(outer.left() + TAB_GAP, outer.top() + TAB_TOP_MARGIN + 1.0),
            pos2(outer.right() - TAB_GAP, outer.bottom() - 2.0),
        );
        let response = ui
            .scope(|ui| {
                ui.visuals_mut().text_edit_bg_color = Some(CONTENT_BG);
                ui.put(
                    rect,
                    egui::TextEdit::singleline(&mut self.app.rename_search_text)
                        .id(egui::Id::new(("rename_search", view_id)))
                        .font(egui::FontId::proportional(14.0))
                        .vertical_align(egui::Align::Center)
                        .margin(egui::Margin::symmetric(TAB_PADDING as i8 - 2, 2)),
                )
            })
            .inner;
        if self.app.rename_request_focus {
            response.request_focus();
            self.app.rename_request_focus = false;
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.app.cancel_rename_search();
        } else if response.lost_focus() {
            self.app.commit_rename_search();
        }
        response
    }
}

impl Behavior<u64> for SearchBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, id: &mut u64) -> UiResponse {
        let Some(index) = search_index(self.app, *id) else {
            return UiResponse::None;
        };
        if index != self.app.state.current_search
            && ui.rect_contains_pointer(ui.max_rect())
            && ui.input(|input| input.pointer.any_pressed())
            && !egui::Popup::is_any_open(ui.ctx())
        {
            self.app.switch_search(index);
        }
        let active = index == self.app.state.current_search;
        let previous = self.app.state.current_search;
        if !active {
            let background = self.inactive.entry(*id).or_default();
            background.selected_result = self.app.state.searches[index].selected_result;
            background.show_results = self.app.state.searches[index].show_results;
            background.swap(self.app);
            self.app.state.current_search = index;
        }
        ui.scope_builder(egui::UiBuilder::new().id(egui::Id::new(("search_pane", *id))), |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                // Keep the hierarchy stable when resizing. Only short panes need
                // outer vertical scrolling; otherwise the table owns that axis.
                egui::ScrollArea::new([true, ui.available_height() < 280.0])
                    .id_salt("search_pane_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(380.0, 280.0));
                        in_process_view::search_area(self.app, ui, active);
                    });
            });
        });
        if !active {
            let gained_focus = ui
                .memory(|memory| memory.focused())
                .and_then(|id| ui.ctx().read_response(id))
                .is_some_and(|response| response.gained_focus() && response.layer_id == ui.layer_id() && ui.max_rect().contains(response.rect.center()));
            let editing = self.app.editing_result.take();
            self.app.state.searches[index].selected_result = self.app.selected_result;
            self.app.state.searches[index].show_results = self.app.state.show_results;
            self.app.changed_addresses.retain(|_, at| at.elapsed() < super::app::CHANGE_HIGHLIGHT);
            self.inactive.get_mut(id).expect("rendered pane state").swap(self.app);
            self.app.state.current_search = previous;
            if gained_focus {
                self.app.switch_search(index);
                self.app.search_value_request_focus = false;
                self.app.editing_result = editing;
            }
        }
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, id: &u64) -> egui::WidgetText {
        search_index(self.app, *id)
            .map(|index| self.app.state.searches[index].description.clone())
            .unwrap_or_default()
            .into()
    }

    fn tab_title_for_tile(&mut self, tiles: &Tiles<u64>, tile_id: TileId) -> egui::WidgetText {
        self.title(tiles, tile_id).into()
    }

    fn tab_ui(&mut self, tiles: &mut Tiles<u64>, ui: &mut egui::Ui, id: egui::Id, tile_id: TileId, state: &egui_tiles::TabState) -> egui::Response {
        let index = tiles.get_pane(&tile_id).and_then(|pane| search_index(self.app, *pane));
        if let Some(index) = index
            && self.app.renaming_search_index == Some(index)
        {
            return self.rename_tab_ui(ui, index);
        }
        let focused = index == Some(self.app.state.current_search);
        let title = self.title(tiles, tile_id);
        let title_galley = ui.painter().layout_no_wrap(title.clone(), egui::FontId::proportional(14.0), BRIGHT_TEXT);
        let search = index.map(|index| &self.app.state.searches[index]);
        let searching = search.is_some_and(|search| search.searching != crate::SearchMode::None);
        let count = search.map_or(0, |search| search.get_result_count());
        let badge = (!searching && count > 0).then(|| ui.painter().layout_no_wrap(compact_count(count), egui::FontId::proportional(11.5), MUTED_TEXT));
        let badge_width = if searching {
            12.0
        } else {
            badge.as_ref().map_or(0.0, |galley| galley.size().x + 12.0)
        };
        // The close button replaces the badge on hover, so both share one slot
        // and the tab width never changes.
        let slot_width = if state.closable { badge_width.max(CLOSE_SIZE + 4.0) } else { badge_width };
        let width = TAB_PADDING
            + title_galley.size().x
            + if slot_width > 0.0 { 7.0 + slot_width } else { 0.0 }
            + if state.closable { TAB_PADDING - 4.0 } else { TAB_PADDING };
        let (outer, _) = ui.allocate_exact_size(vec2(width + TAB_GAP * 2.0, TAB_BAR_HEIGHT), egui::Sense::hover());
        let rect = Rect::from_min_max(
            pos2(outer.left() + TAB_GAP, outer.top() + TAB_TOP_MARGIN),
            pos2(outer.right() - TAB_GAP, outer.bottom()),
        );
        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, state.active, &title));
        if state.is_being_dragged {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                CornerRadius {
                    nw: TAB_RADIUS,
                    ne: TAB_RADIUS,
                    sw: 0,
                    se: 0,
                },
                Stroke::new(1.0, CARD_BORDER),
                StrokeKind::Inside,
            );
            return self.on_tab_button(tiles, tile_id, response);
        }

        let hovered = ui.rect_contains_pointer(rect);
        let corners = CornerRadius {
            nw: TAB_RADIUS,
            ne: TAB_RADIUS,
            sw: 0,
            se: 0,
        };
        // The visible tab merges into its pane; others sit on the strip.
        let (fill, text_color) = if state.active {
            (CONTENT_BG, BRIGHT_TEXT)
        } else if hovered {
            (Color32::from_rgb(32, 36, 42), Color32::from_rgb(210, 214, 220))
        } else {
            (Color32::TRANSPARENT, MUTED_TEXT)
        };
        ui.painter().rect_filled(rect, corners, fill);
        if state.active {
            let edge = if focused { ui.visuals().selection.bg_fill } else { CARD_BORDER };
            ui.painter().rect_stroke(
                rect.expand2(vec2(0.0, 1.0)).translate(vec2(0.0, 1.0)),
                corners,
                Stroke::new(1.0, CARD_BORDER),
                StrokeKind::Inside,
            );
            // Erase the shared edge so tab and pane form one surface.
            ui.painter().line_segment(
                [pos2(rect.left() + 1.0, rect.bottom() + 0.5), pos2(rect.right() - 1.0, rect.bottom() + 0.5)],
                Stroke::new(1.5, CONTENT_BG),
            );
            ui.painter().line_segment(
                [pos2(rect.left() + 3.0, rect.top() + 1.0), pos2(rect.right() - 3.0, rect.top() + 1.0)],
                Stroke::new(2.0, edge),
            );
        }

        let x = rect.left() + TAB_PADDING;
        let center_y = rect.center().y + 0.5;
        let title_size = title_galley.size();
        ui.painter().galley(pos2(x, center_y - title_size.y / 2.0), title_galley, text_color);
        let slot = Rect::from_min_size(pos2(x + title_size.x + 7.0, center_y - 10.0), vec2(slot_width, 20.0));
        let show_close = state.closable && (hovered || (state.active && focused));
        if show_close {
            let close_rect = Rect::from_center_size(
                pos2(slot.right() - (CLOSE_SIZE + 4.0) / 2.0, center_y),
                vec2(CLOSE_SIZE + 4.0, CLOSE_SIZE + 4.0),
            );
            let close = ui
                .interact(close_rect, id.with("close"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(fl!(crate::LANGUAGE_LOADER, "close-tab-hover-text"));
            close.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, fl!(crate::LANGUAGE_LOADER, "close-search-menu")));
            if close.hovered() {
                ui.painter().rect_filled(close_rect, CornerRadius::same(4), Color32::from_white_alpha(24));
            }
            let glyph = close_rect.shrink(6.0);
            let stroke = Stroke::new(1.4, if close.hovered() { BRIGHT_TEXT } else { text_color });
            ui.painter().line_segment([glyph.left_top(), glyph.right_bottom()], stroke);
            ui.painter().line_segment([glyph.right_top(), glyph.left_bottom()], stroke);
            if close.clicked() {
                self.on_tab_close(tiles, tile_id);
            }
        } else if searching {
            let spinner = Rect::from_center_size(pos2(slot.left() + 6.0, center_y), vec2(12.0, 12.0));
            if !super::refresh::backgrounded(ui.ctx()) {
                egui::Spinner::new().color(crate::ui::theme::ACCENT_TEXT).paint_at(ui, spinner);
            }
        } else if let Some(galley) = badge {
            let pill = Rect::from_min_size(pos2(slot.left(), center_y - 9.0), vec2(badge_width, 18.0));
            let pill_fill = if state.active {
                Color32::from_rgb(38, 43, 51)
            } else {
                Color32::from_rgb(30, 34, 40)
            };
            ui.painter().rect_filled(pill, CornerRadius::same(9), pill_fill);
            let galley_size = galley.size();
            ui.painter().galley(pill.center() - galley_size / 2.0, galley, MUTED_TEXT);
        }
        if state.closable && response.clicked_by(egui::PointerButton::Middle) {
            self.on_tab_close(tiles, tile_id);
        }
        self.on_tab_button(tiles, tile_id, response)
    }

    fn drag_ui(&mut self, tiles: &Tiles<u64>, ui: &mut egui::Ui, tile_id: TileId) {
        egui::Frame::new()
            .fill(STRIP_BG)
            .stroke(Stroke::new(1.0, ui.visuals().selection.bg_fill))
            .corner_radius(CornerRadius::same(TAB_RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .shadow(ui.visuals().popup_shadow)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(self.title(tiles, tile_id)).size(14.0).color(BRIGHT_TEXT));
            });
    }

    fn is_tab_closable(&self, tiles: &Tiles<u64>, tile_id: TileId) -> bool {
        self.app.state.searches.len() > 1 && tiles.get_pane(&tile_id).is_some()
    }

    fn on_tab_close(&mut self, tiles: &mut Tiles<u64>, tile_id: TileId) -> bool {
        if let Some(id) = tiles.get_pane(&tile_id) {
            self.action = Some(TabAction::Close(*id));
        }
        // The App releases freezes and cancels workers before sync removes the tile.
        false
    }

    fn on_tab_button(&mut self, tiles: &mut Tiles<u64>, tile_id: TileId, response: egui::Response) -> egui::Response {
        let Some(&id) = tiles.get_pane(&tile_id) else {
            return response;
        };
        if response.clicked() {
            self.action = Some(TabAction::Select(id));
        }
        if response.double_clicked() && self.app.state.searches[self.app.state.current_search].view_id == id {
            self.action = Some(TabAction::Rename(id));
        }
        response.context_menu(|ui| {
            if ui.button(fl!(crate::LANGUAGE_LOADER, "rename-search-menu")).clicked() {
                self.action = Some(TabAction::Rename(id));
                ui.close();
            }
            ui.separator();
            let many = self.app.state.searches.len() > 1;
            if ui
                .add_enabled(many, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "close-search-menu")))
                .clicked()
            {
                self.action = Some(TabAction::Close(id));
                ui.close();
            }
            if ui
                .add_enabled(many, egui::Button::new(fl!(crate::LANGUAGE_LOADER, "close-other-searches-menu")))
                .clicked()
            {
                self.action = Some(TabAction::CloseOthers(id));
                ui.close();
            }
        });
        response.on_hover_text(format!(
            "{}\n{}",
            fl!(crate::LANGUAGE_LOADER, "rename-search-hint"),
            fl!(crate::LANGUAGE_LOADER, "search-tab-drag-hint")
        ))
    }

    fn tab_bar_left_ui(&mut self, tiles: &Tiles<u64>, ui: &mut egui::Ui, tile_id: TileId, _tabs: &egui_tiles::Tabs) {
        // Painted before the tabs, so the card sits behind both strip and pane.
        if let Some(rect) = tiles.rect(tile_id) {
            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_filled(rect, CornerRadius::same(CARD_RADIUS), CONTENT_BG);
            let strip = Rect::from_min_size(rect.min, vec2(rect.width(), TAB_BAR_HEIGHT));
            painter.rect_filled(
                strip,
                CornerRadius {
                    nw: CARD_RADIUS,
                    ne: CARD_RADIUS,
                    sw: 0,
                    se: 0,
                },
                STRIP_BG,
            );
            painter.line_segment(
                [pos2(strip.left(), strip.bottom() + 0.5), pos2(strip.right(), strip.bottom() + 0.5)],
                Stroke::new(1.0, CARD_BORDER),
            );
        }
        ui.add_space(6.0);
    }

    fn tab_bar_trailing_ui(&mut self, _tiles: &Tiles<u64>, ui: &mut egui::Ui, tile_id: TileId, _tabs: &egui_tiles::Tabs) {
        ui.add_space(4.0);
        let (outer, _) = ui.allocate_exact_size(vec2(28.0, TAB_BAR_HEIGHT), egui::Sense::hover());
        let rect = Rect::from_center_size(pos2(outer.center().x, outer.center().y + TAB_TOP_MARGIN / 2.0), vec2(26.0, 24.0));
        let response = ui
            .interact(rect, ui.id().with(("new_search", tile_id)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(fl!(crate::LANGUAGE_LOADER, "open-tab-hover-text"));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, fl!(crate::LANGUAGE_LOADER, "open-tab-hover-text")));
        if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(36, 40, 46));
        }
        let color = if response.hovered() { BRIGHT_TEXT } else { MUTED_TEXT };
        ui.painter()
            .text(rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(18.0), color);
        if response.clicked() {
            self.action = Some(TabAction::New(tile_id));
        }
    }

    fn paint_on_top_of_tile(&self, painter: &egui::Painter, style: &egui::Style, tile_id: TileId, rect: Rect) {
        let Some(&focused) = self.groups.get(&tile_id) else {
            return;
        };
        let stroke = if focused && self.highlight_focus {
            Stroke::new(1.5, style.visuals.selection.bg_fill)
        } else {
            Stroke::new(1.0, CARD_BORDER)
        };
        painter.rect_stroke(rect, CornerRadius::same(CARD_RADIUS), stroke, StrokeKind::Inside);
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        simplification_options()
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        TAB_BAR_HEIGHT
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        Color32::TRANSPARENT
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        8.0
    }

    fn grid_auto_column_count(&self, visible: usize, _rect: Rect, _gap: f32) -> usize {
        (visible as f32).sqrt().ceil().max(1.0) as usize
    }

    fn min_size(&self) -> f32 {
        120.0
    }

    fn resize_stroke(&self, style: &egui::Style, resize_state: egui_tiles::ResizeState) -> Stroke {
        match resize_state {
            egui_tiles::ResizeState::Idle => Stroke::NONE,
            egui_tiles::ResizeState::Hovering => Stroke::new(2.0, crate::ui::theme::ACCENT_TEXT),
            egui_tiles::ResizeState::Dragging => Stroke::new(2.0, style.visuals.selection.bg_fill),
        }
    }

    fn dragged_overlay_color(&self, _visuals: &egui::Visuals) -> Color32 {
        WORKSPACE_BG.gamma_multiply(0.6)
    }

    fn paint_drag_preview(&self, visuals: &egui::Visuals, painter: &egui::Painter, parent_rect: Option<Rect>, preview_rect: Rect) {
        let accent = visuals.selection.bg_fill;
        if let Some(parent_rect) = parent_rect {
            painter.rect_stroke(
                parent_rect,
                CornerRadius::same(CARD_RADIUS),
                Stroke::new(1.0, accent.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
        }
        painter.rect(
            preview_rect.shrink(2.0),
            CornerRadius::same(CARD_RADIUS),
            accent.gamma_multiply(0.22),
            Stroke::new(2.0, accent),
            StrokeKind::Inside,
        );
    }
}

fn simplification_options() -> egui_tiles::SimplificationOptions {
    egui_tiles::SimplificationOptions {
        all_panes_must_have_tabs: true,
        ..Default::default()
    }
}

/// Temporarily lends the existing single-search view its own interaction/cache
/// state when painting an unfocused pane. It must never borrow the focused edit.
struct InactiveSearchView {
    selected_result: Option<SearchResult>,
    editing_result: Option<(usize, String)>,
    result_selection_request_scroll: bool,
    result_edit_request_focus: bool,
    hovered_result_row: Option<usize>,
    search_value_request_focus: bool,
    search_value_select_all: bool,
    show_results: bool,
    address_editor: Option<super::address_editor::AddressEditor>,
    value_change_tracker: ValueCache<Vec<u8>>,
    changed_addresses: HashMap<usize, std::time::Instant>,
}

impl Default for InactiveSearchView {
    fn default() -> Self {
        Self {
            selected_result: None,
            editing_result: None,
            result_selection_request_scroll: false,
            result_edit_request_focus: false,
            hovered_result_row: None,
            search_value_request_focus: false,
            search_value_select_all: false,
            show_results: false,
            address_editor: None,
            value_change_tracker: ValueCache::new(8192),
            changed_addresses: HashMap::new(),
        }
    }
}

impl InactiveSearchView {
    fn swap(&mut self, app: &mut App) {
        macro_rules! swap {
            ($($field:ident),+ $(,)?) => {
                $(std::mem::swap(&mut self.$field, &mut app.$field);)+
            };
        }
        swap!(
            selected_result,
            editing_result,
            result_selection_request_scroll,
            result_edit_request_focus,
            hovered_result_row,
            search_value_request_focus,
            search_value_select_all,
            address_editor,
            value_change_tracker,
            changed_addresses,
        );
        std::mem::swap(&mut self.show_results, &mut app.state.show_results);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SearchContext, SearchType};

    #[test]
    fn repaint_policy_includes_visible_inactive_values_but_not_hidden_tabs() {
        let mut app = App::default();
        app.state.searches[0].set_cached_results(vec![SearchResult::new(0x1000, SearchType::Int)]);
        app.new_search();
        let mut workspace = SearchWorkspace::default();
        workspace.sync(&app);
        assert!(!workspace.has_live_values(&app.state), "The result pane is hidden behind an empty tab");
        workspace.arrange(ContainerKind::Horizontal);
        workspace.tree.simplify(&simplification_options());
        assert!(workspace.has_live_values(&app.state), "Visible inactive results need live repaints");
        app.state.searches[0].searching = crate::SearchMode::Memory;
        assert!(!workspace.has_live_values(&app.state));
        app.state.searches[0].searching = crate::SearchMode::None;
        app.state.searches[0].set_cached_results((1..=1001).map(|i| SearchResult::new(i * 8, SearchType::Int)).collect());
        assert!(!workspace.has_live_values(&app.state), "Collapsed results do not need live repaints");
        app.state.searches[0].show_results = true;
        assert!(workspace.has_live_values(&app.state));
    }

    #[test]
    fn current_layout_tracks_presets_and_manual_docking() {
        let mut app = App::default();
        app.new_search();
        app.new_search();
        let mut workspace = SearchWorkspace::default();
        workspace.sync(&app);
        assert_eq!(workspace.current_layout(), Some(ContainerKind::Tabs));
        for kind in [ContainerKind::Horizontal, ContainerKind::Vertical, ContainerKind::Grid, ContainerKind::Tabs] {
            workspace.arrange(kind);
            workspace.tree.simplify(&simplification_options());
            assert_eq!(workspace.current_layout(), Some(kind));
        }
        workspace.arrange(ContainerKind::Horizontal);
        workspace.tree.simplify(&simplification_options());
        // Group two searches in one card: no longer a preset.
        let Some(Tile::Container(Container::Linear(linear))) = workspace.tree.tiles.get(workspace.tree.root.unwrap()) else {
            panic!("expected a linear root");
        };
        let (first, second) = (linear.children[0], linear.children[1]);
        let Some(Tile::Container(Container::Tabs(tabs))) = workspace.tree.tiles.get(second) else {
            panic!("expected a tab group");
        };
        let moved = tabs.children[0];
        workspace.tree.move_tile_to_container(moved, first, 1, false);
        workspace.tree.simplify(&simplification_options());
        assert_eq!(workspace.current_layout(), None);
    }

    #[test]
    fn compact_counts_stay_short() {
        assert_eq!(compact_count(7), "7");
        assert_eq!(compact_count(9_999), "9999");
        assert_eq!(compact_count(12_345), "12k");
        assert_eq!(compact_count(2_500_000), "2.5M");
    }

    #[test]
    fn replacing_searches_removes_stale_tiles_and_cached_values() {
        let mut app = App::default();
        app.new_search();
        let mut workspace = SearchWorkspace::default();
        workspace.sync(&app);
        workspace.arrange(ContainerKind::Horizontal);
        let old_id = app.state.searches[0].view_id;
        workspace.inactive.insert(old_id, InactiveSearchView::default());
        app.state.searches = vec![Box::new(SearchContext::new("Replacement".to_owned()))];
        app.state.current_search = 0;
        workspace.sync(&app);
        assert!(workspace.tree.tiles.find_pane(&old_id).is_none());
        assert!(workspace.inactive.is_empty());
        assert_eq!(workspace.visible_searches(), vec![app.state.searches[0].view_id]);
        assert_eq!(workspace.tree.tiles.iter().filter(|(_, tile)| matches!(tile, Tile::Pane(_))).count(), 1);
    }
}
