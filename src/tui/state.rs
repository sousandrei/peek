//! Selection, focus, and presentation state for the image explorer.

use std::collections::BTreeSet;

use ratatui::{layout::Rect, widgets::ListState};
use regex::{Regex, RegexBuilder};

use crate::oci::Analysis;

use super::tree::{SortOrder, Tree, parent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Layers,
    Files,
    Command,
}

#[derive(Clone, Copy)]
pub enum Action {
    Category(usize),
    Compare,
    Sort,
    Attributes,
    Help,
}

#[derive(Default)]
pub struct Areas {
    pub layers: Rect,
    pub files: Rect,
    pub command: Rect,
    pub buttons: Vec<(Rect, Action)>,
}

pub struct App<'a> {
    pub analysis: &'a Analysis,
    pub layer: usize,
    pub focus: Focus,
    pub tree: Tree,
    pub visible: Vec<String>,
    pub layers: ListState,
    pub files: ListState,
    pub collapsed: BTreeSet<String>,
    pub categories: [bool; 4],
    pub aggregate: bool,
    pub sort: SortOrder,
    pub attributes: bool,
    pub filter: String,
    pub filter_edit: bool,
    pub filter_error: Option<String>,
    regex: Option<Regex>,
    pub command_scroll: u16,
    pub command_limit: u16,
    pub wrap: bool,
    pub help: bool,
    pub quit: bool,
    pub areas: Areas,
}

impl<'a> App<'a> {
    pub fn new(analysis: &'a Analysis) -> Self {
        let layer = analysis.layers.len().saturating_sub(1);
        let tree = Tree::build(analysis, layer, false);
        let collapsed = tree
            .nodes
            .values()
            .filter(|node| node.directory && node.depth() >= 2)
            .map(|node| node.path.clone())
            .collect();
        let mut app = Self {
            analysis,
            layer,
            focus: Focus::Layers,
            tree,
            visible: Vec::new(),
            layers: ListState::default().with_selected(if analysis.layers.is_empty() {
                None
            } else {
                Some(layer)
            }),
            files: ListState::default(),
            collapsed,
            categories: [true; 4],
            aggregate: false,
            sort: SortOrder::default(),
            attributes: true,
            filter: String::new(),
            filter_edit: false,
            filter_error: None,
            regex: None,
            command_scroll: 0,
            command_limit: 0,
            wrap: true,
            help: false,
            quit: false,
            areas: Areas::default(),
        };
        app.refresh();
        app
    }

    pub fn selected_path(&self) -> Option<&str> {
        self.files
            .selected()
            .and_then(|index| self.visible.get(index))
            .map(String::as_str)
    }

    pub fn refresh(&mut self) {
        let path = self.selected_path().map(str::to_owned);
        self.visible = if self.filter_error.is_some() {
            Vec::new()
        } else {
            self.tree.visible(
                self.categories,
                self.regex.as_ref(),
                &self.collapsed,
                self.sort,
            )
        };
        let selected = path
            .and_then(|path| self.visible.iter().position(|value| *value == path))
            .or_else(|| {
                if self.visible.is_empty() {
                    None
                } else {
                    Some(
                        self.files
                            .selected()
                            .unwrap_or(0)
                            .min(self.visible.len() - 1),
                    )
                }
            });
        self.files.select(selected);
    }

    pub fn select_layer(&mut self, layer: usize) {
        if self.analysis.layers.is_empty() {
            return;
        }
        let layer = layer.min(self.analysis.layers.len() - 1);
        if self.layer == layer {
            return;
        }
        self.layer = layer;
        self.layers.select(Some(layer));
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.tree = Tree::build(self.analysis, self.layer, self.aggregate);
        self.command_scroll = 0;
        self.refresh();
    }

    pub fn move_selection(&mut self, delta: isize) {
        match self.focus {
            Focus::Layers => self.select_layer(self.layer.saturating_add_signed(delta)),
            Focus::Files => {
                if !self.visible.is_empty() {
                    let index = self
                        .files
                        .selected()
                        .unwrap_or(0)
                        .saturating_add_signed(delta)
                        .min(self.visible.len() - 1);
                    self.files.select(Some(index));
                }
            }
            Focus::Command => {
                self.command_scroll = self
                    .command_scroll
                    .saturating_add_signed(delta.clamp(i16::MIN as isize, i16::MAX as isize) as i16)
                    .min(self.command_limit)
            }
        }
    }

    pub fn toggle_main_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Layers => Focus::Files,
            Focus::Files | Focus::Command => Focus::Layers,
        };
    }

    pub fn fold_selected(&mut self) {
        let Some(path) = self.selected_path().map(str::to_owned) else {
            return;
        };
        if self.tree.nodes[&path].directory {
            if !self.collapsed.remove(&path) {
                self.collapsed.insert(path);
            }
            self.refresh();
        }
    }

    pub fn tree_left(&mut self) {
        let Some(path) = self.selected_path().map(str::to_owned) else {
            return;
        };
        if self.tree.nodes[&path].directory && !self.collapsed.contains(&path) {
            self.fold_selected();
        } else if let Some(index) = self
            .visible
            .iter()
            .position(|candidate| candidate == parent(&path))
        {
            self.files.select(Some(index));
        }
    }

    pub fn tree_right(&mut self) {
        let Some(path) = self.selected_path().map(str::to_owned) else {
            return;
        };
        if self.collapsed.remove(&path) {
            self.refresh();
        } else {
            self.move_selection(1);
        }
    }

    pub fn fold_all(&mut self) {
        if self.collapsed.is_empty() {
            self.collapsed = self
                .tree
                .nodes
                .values()
                .filter(|node| node.directory && node.path != "/")
                .map(|node| node.path.clone())
                .collect();
        } else {
            self.collapsed.clear();
        }
        self.refresh();
    }

    pub fn update_filter(&mut self) {
        let compiled = RegexBuilder::new(&self.filter)
            .case_insensitive(true)
            .build();
        match compiled {
            Ok(regex) => {
                self.regex = if self.filter.is_empty() {
                    None
                } else {
                    Some(regex)
                };
                self.filter_error = None;
            }
            Err(error) => {
                self.regex = None;
                self.filter_error = Some(error.to_string());
            }
        }
        self.refresh();
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.filter_edit = false;
        self.update_filter();
    }

    pub fn action(&mut self, action: Action) {
        match action {
            Action::Category(index) => {
                self.categories[index] = !self.categories[index];
                self.refresh();
            }
            Action::Compare => {
                self.aggregate = !self.aggregate;
                self.rebuild();
            }
            Action::Sort => {
                self.sort = self.sort.next();
                self.refresh();
            }
            Action::Attributes => self.attributes = !self.attributes,
            Action::Help => self.help = !self.help,
        }
    }
}
