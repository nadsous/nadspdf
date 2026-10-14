//! Tab groups, as in Chrome: the user gathers tabs under a coloured, optionally named chip in the
//! tab strip, and folds a group away to keep a crowded strip short. Nothing is grouped
//! automatically. Groups belong to the open tabs, and come back with the last session.

use std::collections::{HashMap, HashSet};

use egui::Color32;
use pdfcraft_engine::DocId;
use serde::Serialize;

/// More groups than anyone makes. Settings are untrusted, so a longer list is cut here.
pub const MAX_GROUPS: usize = 64;
/// The longest group name kept, in characters.
pub const MAX_NAME: usize = 48;

/// The group colours, readable on the light and the dark theme.
pub const COLORS: [Color32; 8] = [
    Color32::from_rgb(0x9A, 0xA0, 0xA6),
    Color32::from_rgb(0x5B, 0x8D, 0xEF),
    Color32::from_rgb(0xEF, 0x3E, 0x36),
    Color32::from_rgb(0xF2, 0xB3, 0x3D),
    Color32::from_rgb(0x3D, 0xB8, 0x6B),
    Color32::from_rgb(0xEC, 0x6F, 0xB0),
    Color32::from_rgb(0xA7, 0x7B, 0xF3),
    Color32::from_rgb(0xF2, 0x8B, 0x30),
];

/// The colour of palette entry `i`; an out-of-range index (from settings) is grey.
pub fn color(i: u8) -> Color32 {
    COLORS.get(usize::from(i)).copied().unwrap_or(Color32::GRAY)
}

/// The translated name of palette entry `i`.
pub fn color_name(i: u8) -> &'static str {
    match i {
        1 => tl!("Blue"),
        2 => tl!("Red"),
        3 => tl!("Yellow"),
        4 => tl!("Green"),
        5 => tl!("Pink"),
        6 => tl!("Purple"),
        7 => tl!("Orange"),
        _ => tl!("Gray"),
    }
}

/// A named, coloured set of tabs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TabGroup {
    pub id: u32,
    /// May be empty: the chip then shows only its colour.
    pub name: String,
    /// An index into [`COLORS`].
    pub color: u8,
    /// Folded: only the chip shows (and the active tab, if it is in the group).
    pub collapsed: bool,
}

impl TabGroup {
    /// Read one saved group, or `None` if it is malformed. The name is capped at [`MAX_NAME`].
    pub(crate) fn from_json(v: &serde_json::Value) -> Option<Self> {
        let id = u32::try_from(v["id"].as_u64()?).ok()?;
        let name = v["name"].as_str().unwrap_or_default().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
        let color = v["color"].as_u64().and_then(|c| u8::try_from(c).ok()).filter(|c| usize::from(*c) < COLORS.len()).unwrap_or(0);
        Some(Self { id, name, color, collapsed: v["collapsed"].as_bool().unwrap_or(false) })
    }
}

/// What the user asked of a group, from its chip or a tab's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupAction {
    New(DocId),
    Add(DocId, u32),
    Remove(DocId),
    Toggle(u32),
    Rename(u32),
    Recolor(u32, u8),
    Ungroup(u32),
    Close(u32),
}

/// One thing in the tab strip, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// A group's chip, by group id.
    Chip(u32),
    /// A document tab, by index into `views`.
    Tab(usize),
}

/// The groups of the open tabs.
#[derive(Debug, Default)]
pub struct TabGroups {
    groups: Vec<TabGroup>,
    members: HashMap<DocId, u32>,
    next: u32,
    /// The group whose name is being typed into its chip.
    pub renaming: Option<u32>,
}

impl TabGroups {
    pub fn groups(&self) -> &[TabGroup] {
        &self.groups
    }

    pub fn get(&self, id: u32) -> Option<&TabGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut TabGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// The group `doc`'s tab is in.
    pub fn group_of(&self, doc: DocId) -> Option<u32> {
        self.members.get(&doc).copied()
    }

    /// The tabs in `group`.
    pub fn members(&self, group: u32) -> Vec<DocId> {
        self.members.iter().filter(|(_, g)| **g == group).map(|(d, _)| *d).collect()
    }

    /// A new, unnamed group holding `doc`, in the colour fewest groups use. `None` once there
    /// are [`MAX_GROUPS`].
    pub fn create(&mut self, doc: DocId) -> Option<u32> {
        self.create_like(doc, None)
    }

    fn create_like(&mut self, doc: DocId, like: Option<&TabGroup>) -> Option<u32> {
        if self.groups.len() >= MAX_GROUPS {
            return None;
        }
        // Ids only need to be unique among at most MAX_GROUPS groups.
        while self.get(self.next).is_some() {
            self.next = self.next.wrapping_add(1);
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        let color = like.map(|g| g.color).unwrap_or_else(|| {
            (0..COLORS.len()).filter_map(|c| u8::try_from(c).ok()).min_by_key(|c| self.groups.iter().filter(|g| g.color == *c).count()).unwrap_or(0)
        });
        self.groups.push(TabGroup {
            id,
            name: like.map(|g| g.name.clone()).unwrap_or_default(),
            color,
            collapsed: like.is_some_and(|g| g.collapsed),
        });
        self.members.insert(doc, id);
        Some(id)
    }

    /// Move `doc`'s tab into `group` (unfolding it, so the tab stays in sight).
    pub fn add(&mut self, doc: DocId, group: u32) {
        if let Some(g) = self.get_mut(group) {
            g.collapsed = false;
            self.members.insert(doc, group);
        }
    }

    /// Take `doc`'s tab out of its group; a group left empty goes.
    pub fn remove(&mut self, doc: DocId) {
        self.members.remove(&doc);
        self.drop_empty();
    }

    /// Dissolve `group`, keeping its tabs open.
    pub fn ungroup(&mut self, group: u32) {
        self.members.retain(|_, g| *g != group);
        self.drop_empty();
    }

    /// Forget tabs that have closed, and groups left empty.
    pub fn prune(&mut self, open: &[DocId]) {
        if self.members.keys().any(|d| !open.contains(d)) {
            self.members.retain(|d, _| open.contains(d));
        }
        self.drop_empty();
    }

    fn drop_empty(&mut self) {
        let used: HashSet<u32> = self.members.values().copied().collect();
        self.groups.retain(|g| used.contains(&g.id));
        if self.renaming.is_some_and(|r| !used.contains(&r)) {
            self.renaming = None;
        }
    }

    /// Put `doc` in a group like `saved` (from the last session). `seen` maps saved ids to the
    /// groups already made for them, so tabs of one saved group meet again.
    pub fn restore(&mut self, doc: DocId, saved: &TabGroup, seen: &mut HashMap<u32, u32>) {
        if let Some(id) = seen.get(&saved.id).copied().filter(|id| self.get(*id).is_some()) {
            self.members.insert(doc, id);
        } else if let Some(id) = self.create_like(doc, Some(saved)) {
            seen.insert(saved.id, id);
        }
    }

    /// The strip left to right for tabs `docs`: each group's chip where its first tab is, then
    /// all its tabs in tab order, hidden while it is folded (except the active tab).
    pub fn layout(&self, docs: &[DocId], active: Option<usize>) -> Vec<Slot> {
        let mut out = Vec::with_capacity(docs.len().saturating_add(self.groups.len()));
        let mut placed = HashSet::new();
        for (i, doc) in docs.iter().enumerate() {
            let Some(group) = self.group_of(*doc).and_then(|g| self.get(g)) else {
                out.push(Slot::Tab(i));
                continue;
            };
            if !placed.insert(group.id) {
                continue;
            }
            out.push(Slot::Chip(group.id));
            for (j, other) in docs.iter().enumerate() {
                if self.group_of(*other) == Some(group.id) && (!group.collapsed || active == Some(j)) {
                    out.push(Slot::Tab(j));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs(n: u64) -> Vec<DocId> {
        (0..n).map(DocId).collect()
    }

    #[test]
    fn grouped_tabs_gather_behind_their_chip_and_fold_away() {
        let d = docs(4);
        let mut g = TabGroups::default();
        let a = g.create(d[1]).unwrap();
        g.add(d[3], a);
        assert_eq!(g.layout(&d, None), [Slot::Tab(0), Slot::Chip(a), Slot::Tab(1), Slot::Tab(3), Slot::Tab(2)]);
        g.get_mut(a).unwrap().collapsed = true;
        assert_eq!(g.layout(&d, Some(0)), [Slot::Tab(0), Slot::Chip(a), Slot::Tab(2)]);
        // The active tab stays in sight inside a folded group.
        assert_eq!(g.layout(&d, Some(3)), [Slot::Tab(0), Slot::Chip(a), Slot::Tab(3), Slot::Tab(2)]);
    }

    #[test]
    fn new_groups_take_unused_colours_and_empty_groups_go() {
        let d = docs(3);
        let mut g = TabGroups::default();
        let a = g.create(d[0]).unwrap();
        let b = g.create(d[1]).unwrap();
        assert_ne!(g.get(a).unwrap().color, g.get(b).unwrap().color);
        g.renaming = Some(b);
        g.remove(d[1]);
        assert!(g.get(b).is_none() && g.renaming.is_none());
        g.add(d[2], a);
        g.prune(&[d[2]]);
        assert_eq!(g.members(a), [d[2]]);
        g.ungroup(a);
        assert!(g.groups().is_empty() && g.group_of(d[2]).is_none());
        // Adding to a group that is gone does nothing.
        g.add(d[2], a);
        assert!(g.group_of(d[2]).is_none());
    }

    #[test]
    fn group_count_is_capped() {
        let mut g = TabGroups::default();
        for i in 0..MAX_GROUPS as u64 {
            assert!(g.create(DocId(i)).is_some());
        }
        assert!(g.create(DocId(999)).is_none());
    }

    #[test]
    fn saved_groups_are_read_leniently_and_restored_together() {
        let bad = serde_json::json!({"id": -1});
        assert!(TabGroup::from_json(&bad).is_none());
        let long = "x".repeat(500);
        let saved = TabGroup::from_json(&serde_json::json!({"id": 7, "name": long, "color": 200, "collapsed": true})).unwrap();
        assert_eq!((saved.name.chars().count(), saved.color, saved.collapsed), (MAX_NAME, 0, true));
        let mut g = TabGroups::default();
        let mut seen = HashMap::new();
        g.restore(DocId(1), &saved, &mut seen);
        g.restore(DocId(2), &saved, &mut seen);
        assert_eq!(g.groups().len(), 1);
        assert_eq!(g.group_of(DocId(1)), g.group_of(DocId(2)));
    }
}
