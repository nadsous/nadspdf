//! NadsPDF: the ink model of craft-libs' craft-prepress (storytold/craft-libs
//! 92d392cff132eaf53e13afff77f38b5c11b73aa0, `crates/prepress/src/ink.rs`, MIT OR Apache-2.0):
//! the document's process and spot inks, their aliases and which print as process colour. Only
//! the data model, without the colour conversions that need print production (separations).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};


/// A process ink, in plate order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Process {
    Cyan,
    Magenta,
    Yellow,
    Black,
}

impl Process {
    /// The four process inks, in plate order.
    pub const ALL: [Process; 4] = [Process::Cyan, Process::Magenta, Process::Yellow, Process::Black];

    /// The colorant name a `Separation` or `DeviceN` color space gives this ink.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cyan => "Cyan",
            Self::Magenta => "Magenta",
            Self::Yellow => "Yellow",
            Self::Black => "Black",
        }
    }

    /// The process ink a colorant name denotes (ISO 32000-2 §8.6.6.4).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The ink's position in a CMYK color.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// How a spot ink looks, for previews and for printing it as process color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alternate {
    /// CMYK components, `0.0..=1.0`.
    Cmyk([f32; 4]),
    /// CIE L\*a\*b\* relative to D50.
    Lab([f32; 3]),
}

/// A spot ink.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    /// The colorant name, as a `Separation` color space would name it, without PDF name
    /// escapes (see [`normalize_name`]).
    pub name: String,
    /// The ink at 100%.
    pub alternate: Alternate,
    /// How opaque the ink is (`0.0..=1.0`, as InDesign's Ink Manager states it), when known.
    pub solidity: Option<f32>,
}

/// A process or spot ink.
#[derive(Clone, Debug, PartialEq)]
pub enum Ink {
    Process(Process),
    Spot(Spot),
}

impl Ink {
    /// The colorant name: `Cyan` … `Black`, or the spot ink's name.
    pub fn name(&self) -> &str {
        match self {
            Self::Process(p) => p.name(),
            Self::Spot(s) => &s.name,
        }
    }

    pub fn is_spot(&self) -> bool {
        matches!(self, Self::Spot(_))
    }
}

/// An ink's index in its [`InkSet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InkId(u16);

impl InkId {
    /// The ink's position in [`InkSet::iter`] order, which is also its plane in
    /// [`Plates::planes`](crate::Plates::planes).
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The id of process ink `p`, the same in every [`InkSet`].
    pub const fn process(p: Process) -> Self {
        Self(p as u16)
    }
}

/// The inks of one document or job: the four process inks, always first and in plate order,
/// then spot inks in the order added. Ids stay valid as inks are added.
///
/// The set is also the job's ink manager, as InDesign's Ink Manager states it: a spot ink can
/// print on another ink's plate ([`alias`](Self::alias)) or as process color
/// ([`set_to_process`](Self::set_to_process), [`set_all_to_process`](Self::set_all_to_process)).
/// [`resolve`](Self::resolve) applies both to a color.
#[derive(Clone, Debug, PartialEq)]
pub struct InkSet {
    inks: Vec<Ink>,
    aliases: BTreeMap<InkId, InkId>,
    to_process: BTreeSet<InkId>,
    all_to_process: bool,
}

impl Default for InkSet {
    fn default() -> Self {
        Self::new()
    }
}

impl InkSet {
    /// The four process inks.
    pub fn new() -> Self {
        Self {
            inks: Process::ALL.into_iter().map(Ink::Process).collect(),
            aliases: BTreeMap::new(),
            to_process: BTreeSet::new(),
            all_to_process: false,
        }
    }

    /// Adds `ink` and returns its id. An ink whose name is already in the set (compared after
    /// [`normalize_name`]) is not added again, and its existing id is returned; a spot named
    /// `Cyan` … `Black` is the process ink. A spot's name is stored normalized. `None` for the
    /// names `All` and `None`, which are not inks (§8.6.6.4), and when the set is full
    /// (65 536 inks).
    pub fn add(&mut self, ink: Ink) -> Option<InkId> {
        let spot = match ink {
            Ink::Process(p) => return Some(InkId::process(p)),
            Ink::Spot(spot) => spot,
        };
        let name = normalize_name(&spot.name).into_owned();
        self.add_decoded(Spot { name, ..spot })
    }

    /// [`add`](Self::add) for a spot whose name is already decoded, as a PDF reader hands it
    /// over: `#` in the name is a literal character.
    pub(crate) fn add_decoded(&mut self, spot: Spot) -> Option<InkId> {
        if spot.name == "All" || spot.name == "None" {
            return None;
        }
        if let Some(id) = self.lookup_decoded(&spot.name) {
            return Some(id);
        }
        let id = InkId(u16::try_from(self.inks.len()).ok()?);
        self.inks.push(Ink::Spot(spot));
        Some(id)
    }

    /// The ink named `name`, compared after [`normalize_name`]: `"/Gold#20Foil"` finds
    /// `Gold Foil`.
    pub fn lookup(&self, name: &str) -> Option<InkId> {
        self.lookup_decoded(&normalize_name(name))
    }

    fn lookup_decoded(&self, name: &str) -> Option<InkId> {
        if let Some(p) = Process::from_name(name) {
            return Some(InkId::process(p));
        }
        let at = self.inks.iter().skip(4).position(|ink| ink.name() == name)?;
        u16::try_from(at + 4).ok().map(InkId)
    }

    pub fn get(&self, id: InkId) -> Option<&Ink> {
        self.inks.get(id.index())
    }

    /// Every ink with its id: the process inks, then the spot inks.
    pub fn iter(&self) -> impl Iterator<Item = (InkId, &Ink)> {
        // At most 65 536 inks (`add`), so every index fits.
        self.inks.iter().enumerate().map(|(i, ink)| (InkId(i as u16), ink))
    }

    /// The spot inks with their ids.
    pub fn spots(&self) -> impl Iterator<Item = (InkId, &Spot)> {
        self.iter().filter_map(|(id, ink)| match ink {
            Ink::Spot(s) => Some((id, s)),
            Ink::Process(_) => None,
        })
    }

    /// The number of inks, process inks included.
    pub fn len(&self) -> usize {
        self.inks.len()
    }

    /// Always `false`: a set holds the process inks.
    pub fn is_empty(&self) -> bool {
        self.inks.is_empty()
    }

    fn spot(&self, id: InkId) -> Option<&Spot> {
        match self.get(id)? {
            Ink::Spot(s) => Some(s),
            Ink::Process(_) => None,
        }
    }

    /// Prints spot ink `from` on ink `to`'s plate. `to` may be a process ink or another spot,
    /// which may itself be aliased or printed as process color; `resolve` follows the chain.
    /// `to == from` removes the alias. Returns `false`, changing nothing, when `from` is not a
    /// spot ink of the set, `to` is not in the set, or the alias would make a loop.
    pub fn alias(&mut self, from: InkId, to: InkId) -> bool {
        if self.spot(from).is_none() || self.get(to).is_none() {
            return false;
        }
        if from == to {
            self.aliases.remove(&from);
            return true;
        }
        if self.plate_chain(to).any(|id| id == from) {
            return false;
        }
        self.aliases.insert(from, to);
        true
    }

    /// The ink spot ink `from` is aliased to, if any.
    pub fn alias_of(&self, from: InkId) -> Option<InkId> {
        self.aliases.get(&from).copied()
    }

    /// Prints spot ink `ink` as process color (`on`), converted from its alternate, or on its
    /// own plate. Returns `false`, changing nothing, when `ink` is not a spot ink of the set.
    pub fn set_to_process(&mut self, ink: InkId, on: bool) -> bool {
        if self.spot(ink).is_none() {
            return false;
        }
        if on {
            self.to_process.insert(ink);
        } else {
            self.to_process.remove(&ink);
        }
        true
    }

    /// Prints every spot ink as process color ("All Spots to Process"), whatever
    /// [`set_to_process`](Self::set_to_process) says for each. Aliases still apply first: a
    /// spot aliased to a process ink prints on that ink's plate.
    pub fn set_all_to_process(&mut self, on: bool) {
        self.all_to_process = on;
    }

    pub fn all_to_process(&self) -> bool {
        self.all_to_process
    }

    /// Whether spot ink `ink` prints as process color when a color names it directly (an
    /// alias of it is followed first, see [`plate_of`](Self::plate_of)).
    pub fn is_to_process(&self, ink: InkId) -> bool {
        self.spot(ink).is_some() && (self.all_to_process || self.to_process.contains(&ink))
    }

    /// `id`, then each ink it is aliased to in turn. Stops after as many steps as there are
    /// inks, so a loop can't make it endless.
    fn plate_chain(&self, id: InkId) -> impl Iterator<Item = InkId> + '_ {
        std::iter::successors(Some(id), |id| self.alias_of(*id)).take(self.inks.len())
    }

    /// The end of `id`'s alias chain: `id` itself when it has no alias. It prints on its own
    /// plate unless [`is_to_process`](Self::is_to_process) says it converts. `None` when `id`
    /// is not in the set.
    pub fn alias_end(&self, id: InkId) -> Option<InkId> {
        self.get(id)?;
        self.plate_chain(id).last()
    }

    /// The ink whose plate a tint of `id` prints on after aliases: `id` itself, the end of its
    /// alias chain, or `None` when that ink prints as process color (or `id` is not in the
    /// set).
    pub fn plate_of(&self, id: InkId) -> Option<InkId> {
        let end = self.alias_end(id)?;
        (!self.is_to_process(end)).then_some(end)
    }

}

/// A colorant name as a PDF name object carries it: without a leading `/` and with `#xx`
/// escapes decoded (ISO 32000-2 §7.3.5), read as UTF-8 (invalid sequences replaced).
pub fn normalize_name(name: &str) -> Cow<'_, str> {
    let name = name.strip_prefix('/').unwrap_or(name);
    if !name.contains('#') {
        return Cow::Borrowed(name);
    }
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'#', Some(decoded)) => {
                out.push(decoded);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}
