//! A tiny layout engine modelled on rofi's box tree, plus the built-in layouts
//! (recreated from adi1090x/rofi, GPL-3.0). Everything is drawn with egui's painter,
//! so the same code renders the real popup, the settings preview and the gallery.

use crate::config::{ButtonShape, LookConfig, Palette, PictureShape};
use crate::icons::{IconCache, IconSrc};
use crate::palette::on_selected;
use eframe::egui::{
    self, text::LayoutJob, text::TextWrapping, Color32, FontFamily, FontId, Id, Margin, Pos2, Rect, RichText, Rounding, Sense,
    Ui, Vec2,
};

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Bg,
    BgAlt,
    Fg,
    Sel,
    Act,
    Urg,
    /// Readable text on top of `Sel`.
    OnSel,
}

#[derive(Clone, Copy)]
pub struct Colors {
    pub bg: Color32,
    pub bg_alt: Color32,
    pub fg: Color32,
    pub sel: Color32,
    pub act: Color32,
    pub urg: Color32,
    pub on_sel: Color32,
    /// Opacity of background panels (lets blur/acrylic show through).
    pub opacity: f32,
}

impl Colors {
    pub fn new(p: &Palette, opacity: f32) -> Self {
        Colors {
            bg: p.background.c32(),
            bg_alt: p.background_alt.c32(),
            fg: p.foreground.c32(),
            sel: p.selected.c32(),
            act: p.active.c32(),
            urg: p.urgent.c32(),
            on_sel: on_selected(p).c32(),
            opacity: opacity.clamp(0.2, 1.0),
        }
    }
    pub fn get(&self, r: Role) -> Color32 {
        match r {
            Role::Bg => self.bg.gamma_multiply(self.opacity),
            Role::BgAlt => self.bg_alt.gamma_multiply((self.opacity + 0.15).min(1.0)),
            Role::Fg => self.fg,
            Role::Sel => self.sel,
            Role::Act => self.act,
            Role::Urg => self.urg,
            Role::OnSel => self.on_sel,
        }
    }
}

/// A colour role plus an alpha multiplier.
#[derive(Clone, Copy, Debug)]
pub struct Fill(pub Role, pub f32);

fn fill(r: Role) -> Option<Fill> {
    Some(Fill(r, 1.0))
}

// ---------------------------------------------------------------------------
// Layout tree
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug)]
pub enum Size {
    /// As big as the content needs.
    Auto,
    /// A fixed length (points at size 1.0) along the parent's direction.
    Fixed(f32),
    /// Share the leftover space by weight.
    Expand(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MsgKind {
    /// "user@pc"
    Header,
    /// Uptime
    Info,
    /// Launcher status line ("Finding your apps…"). Hidden when empty.
    Status,
}

#[derive(Clone, Debug)]
pub struct ElStyle {
    /// Icon above the text (grid tiles) instead of beside it.
    pub vertical: bool,
    pub pad: Margin,
    pub round: Rounding,
    pub icon: f32,
    pub gap: f32,
    pub fill: Option<Fill>,
    pub sel_fill: Option<Fill>,
    pub text: Role,
    pub sel_text: Role,
    pub label: bool,
    pub font: f32,
}

#[derive(Clone, Debug)]
pub enum Node {
    Stack {
        dir: Dir,
        size: Size,
        pad: Margin,
        gap: f32,
        fill: Option<Fill>,
        round: Rounding,
        /// Draw the user's picture here; the value is how much of the background
        /// colour to lay over it (0 = pure picture).
        image: Option<f32>,
        children: Vec<Node>,
    },
    Search {
        size: Size,
        pad: Margin,
        fill: Option<Fill>,
        round: Rounding,
        text: Role,
    },
    Modes {
        size: Size,
        gap: f32,
        pad: Margin,
        fill: Option<Fill>,
        round: Rounding,
        sel_fill: Fill,
        text: Role,
        sel_text: Role,
        labels: bool,
    },
    Message {
        size: Size,
        kind: MsgKind,
        pad: Margin,
        fill: Option<Fill>,
        round: Rounding,
        text: Role,
        center: bool,
    },
    List {
        size: Size,
        cols: usize,
        lines: usize,
        gap: f32,
        el: ElStyle,
    },
    Spacer(Size),
    /// Picture in a circle/polygon (`shape` sides, 0 = circle) with results on a ring band
    /// around it and the mode buttons on an outer ring. Fills its whole box.
    Rings {
        shape: u8,
        radius: f32,
        power: bool,
    },
    /// Picture badge in a circle/polygon with a flat menu panel (`child`) coming out of its side.
    Panel {
        shape: u8,
        radius: f32,
        child: Box<Node>,
    },
}

/// Outline of the whole popup window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    /// Normal rectangle (corners come from the root box's rounding).
    Rect,
    /// Rounded dome on top, flat sides and bottom.
    Arch,
    /// Circle (0) or regular polygon with this many sides, flat edge at the bottom.
    Polygon(u8),
}

pub struct Layout {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub width: f32,
    pub root: Node,
    pub outline: Outline,
    /// Fixed height (needed for circles); None = as tall as the content.
    pub height: Option<f32>,
}

impl Node {
    fn size(&self) -> Size {
        match self {
            Node::Stack { size, .. }
            | Node::Search { size, .. }
            | Node::Modes { size, .. }
            | Node::Message { size, .. }
            | Node::List { size, .. }
            | Node::Spacer(size) => *size,
            Node::Rings { .. } | Node::Panel { .. } => Size::Auto,
        }
    }

    /// Visit every List node (used to know how many rows/columns are visible).
    pub fn find_list(&self) -> Option<(usize, usize)> {
        match self {
            Node::List { cols, lines, .. } => Some((*cols, *lines)),
            Node::Stack { children, .. } => children.iter().find_map(|c| c.find_list()),
            Node::Rings { shape, radius, power } => Some((1, ring_capacity(*shape, *radius, *power))),
            Node::Panel { child, .. } => child.find_list(),
            _ => None,
        }
    }

    /// Items laid out around a ring: arrow keys left/right should move through them.
    pub fn is_ring(&self) -> bool {
        matches!(self, Node::Rings { .. })
    }

    pub fn has_search(&self) -> bool {
        match self {
            Node::Search { .. } => true,
            Node::Stack { children, .. } => children.iter().any(|c| c.has_search()),
            Node::Rings { power, .. } => !power,
            Node::Panel { child, .. } => child.has_search(),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Builders (keep the preset definitions readable)
// ---------------------------------------------------------------------------

fn m(v: f32) -> Margin {
    Margin::same(v)
}
fn mxy(x: f32, y: f32) -> Margin {
    Margin::symmetric(x, y)
}
fn r(v: f32) -> Rounding {
    Rounding::same(v)
}
fn r4(nw: f32, ne: f32, sw: f32, se: f32) -> Rounding {
    Rounding { nw, ne, sw, se }
}
const PILL: f32 = 999.0;

fn stack(dir: Dir, size: Size, pad: Margin, gap: f32, children: Vec<Node>) -> Node {
    Node::Stack { dir, size, pad, gap, fill: None, round: Rounding::ZERO, image: None, children }
}
fn with_fill(mut n: Node, f: Fill, rd: Rounding) -> Node {
    if let Node::Stack { fill, round, .. } = &mut n {
        *fill = Some(f);
        *round = rd;
    }
    n
}
fn with_image(mut n: Node, dim: f32, rd: Rounding) -> Node {
    if let Node::Stack { image, round, .. } = &mut n {
        *image = Some(dim);
        *round = rd;
    }
    n
}

fn row_el(pad: Margin, round: Rounding, icon: f32, sel: Role, sel_text: Role) -> ElStyle {
    ElStyle {
        vertical: false,
        pad,
        round,
        icon,
        gap: 14.0,
        fill: None,
        sel_fill: fill(sel),
        text: Role::Fg,
        sel_text,
        label: true,
        font: 1.0,
    }
}

// ---------------------------------------------------------------------------
// Built-in launcher layouts
// ---------------------------------------------------------------------------

pub fn launcher_layouts() -> Vec<Layout> {
    let modes = |pad: Margin, gap: f32, rd: Rounding, labels: bool| Node::Modes {
        size: Size::Auto,
        gap,
        pad,
        fill: fill(Role::BgAlt),
        round: rd,
        sel_fill: Fill(Role::Sel, 1.0),
        text: Role::Fg,
        sel_text: Role::OnSel,
        labels,
    };
    let search =
        |pad: Margin, f: Option<Fill>, rd: Rounding, text: Role, size: Size| Node::Search { size, pad, fill: f, round: rd, text };
    let status = Node::Message {
        size: Size::Auto,
        kind: MsgKind::Status,
        pad: m(12.0),
        fill: fill(Role::BgAlt),
        round: r(10.0),
        text: Role::Fg,
        center: false,
    };

    // Type 6: picture on the left with search + mode buttons, list on the right.
    let hearth = |mirror: bool| {
        let rd_img = if mirror { r4(0.0, 15.0, 0.0, 15.0) } else { r4(15.0, 0.0, 15.0, 0.0) };
        let image_side = with_image(
            stack(
                Dir::Column,
                Size::Expand(1.0),
                m(20.0),
                0.0,
                vec![
                    search(m(15.0), fill(Role::BgAlt), r(10.0), Role::Fg, Size::Auto),
                    Node::Spacer(Size::Expand(1.0)),
                    modes(mxy(8.0, 15.0), 16.0, r(10.0), true),
                ],
            ),
            0.0,
            rd_img,
        );
        let list_side = stack(
            Dir::Column,
            Size::Expand(1.0),
            m(20.0),
            20.0,
            vec![
                status.clone(),
                Node::List {
                    size: Size::Auto,
                    cols: 1,
                    lines: 8,
                    gap: 10.0,
                    el: row_el(m(8.0), r(10.0), 32.0, Role::Sel, Role::OnSel),
                },
            ],
        );
        let children = if mirror { vec![list_side, image_side] } else { vec![image_side, list_side] };
        with_fill(stack(Dir::Row, Size::Auto, m(0.0), 0.0, children), Fill(Role::Bg, 1.0), r(15.0))
    };

    // Type 7: wide picture banner on top holding the search pill and round mode buttons.
    let banner = with_fill(
        stack(
            Dir::Column,
            Size::Auto,
            m(0.0),
            0.0,
            vec![
                with_image(
                    stack(
                        Dir::Row,
                        Size::Auto,
                        mxy(50.0, 70.0),
                        10.0,
                        vec![
                            search(mxy(16.0, 12.0), fill(Role::BgAlt), r(PILL), Role::Fg, Size::Fixed(280.0)),
                            Node::Spacer(Size::Expand(1.0)),
                            modes(m(12.0), 10.0, r(PILL), false),
                        ],
                    ),
                    0.0,
                    r4(20.0, 20.0, 0.0, 0.0),
                ),
                stack(
                    Dir::Column,
                    Size::Auto,
                    m(20.0),
                    16.0,
                    vec![
                        status.clone(),
                        Node::List {
                            size: Size::Auto,
                            cols: 1,
                            lines: 7,
                            gap: 8.0,
                            el: row_el(mxy(10.0, 4.0), r(PILL), 32.0, Role::Sel, Role::OnSel),
                        },
                    ],
                ),
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(20.0),
    );

    // Type 3: big icon grid over a dimmed picture.
    let grid = with_image(
        stack(
            Dir::Column,
            Size::Auto,
            m(20.0),
            20.0,
            vec![
                search(m(15.0), Some(Fill(Role::BgAlt, 0.75)), r(10.0), Role::Fg, Size::Auto),
                status.clone(),
                Node::List {
                    size: Size::Auto,
                    cols: 5,
                    lines: 3,
                    gap: 4.0,
                    el: ElStyle {
                        vertical: true,
                        pad: mxy(10.0, 18.0),
                        round: r(10.0),
                        icon: 56.0,
                        gap: 12.0,
                        fill: None,
                        sel_fill: Some(Fill(Role::Sel, 0.9)),
                        text: Role::Fg,
                        sel_text: Role::OnSel,
                        label: true,
                        font: 0.9,
                    },
                },
            ],
        ),
        0.78,
        r(14.0),
    );

    // Type 2: small and simple; coloured search bar on top of a list.
    let compact = with_fill(
        stack(
            Dir::Column,
            Size::Auto,
            Margin { left: 0.0, right: 0.0, top: 0.0, bottom: 8.0 },
            6.0,
            vec![
                search(m(15.0), fill(Role::Sel), r4(12.0, 12.0, 0.0, 0.0), Role::OnSel, Size::Auto),
                Node::List {
                    size: Size::Auto,
                    cols: 1,
                    lines: 6,
                    gap: 4.0,
                    el: ElStyle {
                        sel_fill: fill(Role::BgAlt),
                        sel_text: Role::Fg,
                        ..row_el(mxy(12.0, 8.0), r(0.0), 32.0, Role::Sel, Role::Fg)
                    },
                },
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(12.0),
    );

    // Type 1: roomy two-column list with mode buttons along the bottom.
    let classic = with_fill(
        stack(
            Dir::Column,
            Size::Auto,
            m(36.0),
            12.0,
            vec![
                search(mxy(14.0, 12.0), fill(Role::BgAlt), r(12.0), Role::Fg, Size::Auto),
                status.clone(),
                Node::List {
                    size: Size::Auto,
                    cols: 2,
                    lines: 9,
                    gap: 5.0,
                    el: row_el(mxy(12.0, 6.0), r(20.0), 26.0, Role::Sel, Role::OnSel),
                },
                modes(mxy(8.0, 10.0), 10.0, r(20.0), true),
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(20.0),
    );

    let v = vec![
        Layout {
            id: "hearth",
            name: "Hearth",
            blurb: "Your picture on the left, apps on the right",
            width: 1000.0,
            root: hearth(false),
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "hearth-right",
            name: "Hearth (mirrored)",
            blurb: "Apps on the left, your picture on the right",
            width: 1000.0,
            root: hearth(true),
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "banner",
            name: "Banner",
            blurb: "A wide picture banner above the list",
            width: 700.0,
            root: banner,
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "grid",
            name: "Grid",
            blurb: "Big icons over a softly dimmed picture",
            width: 760.0,
            root: grid,
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "compact",
            name: "Compact",
            blurb: "Small and simple",
            width: 420.0,
            root: compact,
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "classic",
            name: "Classic",
            blurb: "Two columns with mode buttons below",
            width: 800.0,
            root: classic,
            outline: Outline::Rect,
            height: None,
        },
    ];
    v
}

// ---------------------------------------------------------------------------
// Built-in power menu layouts (type 6, styles 1-5)
// ---------------------------------------------------------------------------

pub fn power_layouts() -> Vec<Layout> {
    let header = |pad: Margin, f: Fill, rd: Rounding, text: Role| Node::Message {
        size: Size::Auto,
        kind: MsgKind::Header,
        pad,
        fill: Some(f),
        round: rd,
        text,
        center: true,
    };
    let info = |pad: Margin, f: Fill, rd: Rounding, text: Role| Node::Message {
        size: Size::Auto,
        kind: MsgKind::Info,
        pad,
        fill: Some(f),
        round: rd,
        text,
        center: true,
    };
    let tile = |pad: Margin, rd: Rounding, icon: f32| ElStyle {
        vertical: true,
        pad,
        round: rd,
        icon,
        gap: 8.0,
        fill: fill(Role::BgAlt),
        sel_fill: fill(Role::Sel),
        text: Role::Fg,
        sel_text: Role::OnSel,
        label: true,
        font: 0.9,
    };
    let rowtile = |pad: Margin, rd: Rounding| ElStyle {
        vertical: false,
        pad,
        round: rd,
        icon: 26.0,
        gap: 14.0,
        fill: fill(Role::BgAlt),
        sel_fill: fill(Role::Sel),
        text: Role::Fg,
        sel_text: Role::OnSel,
        label: true,
        font: 1.0,
    };

    let picture =
        |size: Size, pad: f32, gap: f32, rd: Rounding, hfill: Fill, htext: Role, ifill: Fill, itext: Role, hr: Rounding| {
            with_image(
                stack(
                    Dir::Column,
                    size,
                    m(pad),
                    gap,
                    vec![header(m(14.0), hfill, hr, htext), Node::Spacer(Size::Expand(1.0)), info(m(14.0), ifill, hr, itext)],
                ),
                0.0,
                rd,
            )
        };
    let list_box = |size: Size, pad: f32, cols: usize, lines: usize, gap: f32, el: ElStyle| {
        stack(Dir::Column, size, m(pad), 0.0, vec![Node::List { size: Size::Auto, cols, lines, gap, el }])
    };

    let s1 = with_fill(
        stack(
            Dir::Row,
            Size::Auto,
            m(0.0),
            0.0,
            vec![
                picture(
                    Size::Expand(1.0),
                    30.0,
                    30.0,
                    r4(15.0, 0.0, 15.0, 0.0),
                    Fill(Role::Urg, 1.0),
                    Role::Fg,
                    Fill(Role::Act, 1.0),
                    Role::Bg,
                    r(10.0),
                ),
                list_box(Size::Expand(1.0), 30.0, 2, 3, 30.0, tile(mxy(10.0, 28.0), r(10.0), 36.0)),
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(15.0),
    );
    let s2 = with_fill(
        stack(
            Dir::Row,
            Size::Auto,
            m(0.0),
            0.0,
            vec![
                picture(
                    Size::Expand(1.0),
                    20.0,
                    20.0,
                    r4(24.0, 0.0, 24.0, 0.0),
                    Fill(Role::Urg, 1.0),
                    Role::Fg,
                    Fill(Role::Act, 1.0),
                    Role::Fg,
                    r(PILL),
                ),
                list_box(Size::Expand(1.2), 20.0, 3, 2, 20.0, tile(mxy(10.0, 30.0), r(PILL), 36.0)),
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(24.0),
    );
    let s3 = with_fill(
        stack(
            Dir::Row,
            Size::Auto,
            m(0.0),
            0.0,
            vec![
                picture(
                    Size::Expand(1.0),
                    30.0,
                    0.0,
                    r4(10.0, 0.0, 10.0, 0.0),
                    Fill(Role::Urg, 1.0),
                    Role::Bg,
                    Fill(Role::Act, 1.0),
                    Role::Fg,
                    r(10.0),
                ),
                list_box(Size::Expand(1.2), 30.0, 3, 2, 30.0, tile(mxy(10.0, 18.0), r(20.0), 36.0)),
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(10.0),
    );
    let s4 = with_fill(
        stack(
            Dir::Row,
            Size::Auto,
            m(0.0),
            0.0,
            vec![
                picture(
                    Size::Fixed(520.0),
                    60.0,
                    0.0,
                    r(0.0),
                    Fill(Role::Urg, 1.0),
                    Role::Bg,
                    Fill(Role::Act, 1.0),
                    Role::Fg,
                    r(0.0),
                ),
                list_box(Size::Expand(1.0), 30.0, 1, 6, 12.0, rowtile(mxy(16.0, 12.0), r(0.0))),
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(0.0),
    );
    let s5 = with_fill(
        stack(
            Dir::Row,
            Size::Auto,
            m(0.0),
            0.0,
            vec![
                list_box(Size::Expand(1.0), 30.0, 2, 3, 20.0, tile(mxy(10.0, 26.0), r(0.0), 34.0)),
                picture(
                    Size::Fixed(460.0),
                    60.0,
                    0.0,
                    r(0.0),
                    Fill(Role::Urg, 1.0),
                    Role::Bg,
                    Fill(Role::Act, 1.0),
                    Role::Fg,
                    r(0.0),
                ),
            ],
        ),
        Fill(Role::Bg, 1.0),
        r(0.0),
    );

    let v = vec![
        Layout {
            id: "hearth",
            name: "Hearth",
            blurb: "Picture with your name and uptime, big tiles",
            width: 800.0,
            root: s1,
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "bubbles",
            name: "Bubbles",
            blurb: "Round, bubbly buttons",
            width: 1000.0,
            root: s2,
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "tiles",
            name: "Tiles",
            blurb: "Three across, two down",
            width: 860.0,
            root: s3,
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "column",
            name: "Column",
            blurb: "Big picture, a tidy list of actions",
            width: 820.0,
            root: s4,
            outline: Outline::Rect,
            height: None,
        },
        Layout {
            id: "column-left",
            name: "Column (mirrored)",
            blurb: "Actions first, picture on the right",
            width: 800.0,
            root: s5,
            outline: Outline::Rect,
            height: None,
        },
    ];
    v
}

// ---------------------------------------------------------------------------
// Shaped windows: arch, circle and polygon layouts. The user picks the shape and
// a menu style (rings around the picture, a box beside it, or everything inside).
// ---------------------------------------------------------------------------

fn search_node(pad: Margin, f: Option<Fill>, rd: Rounding, size: Size) -> Node {
    Node::Search { size, pad, fill: f, round: rd, text: Role::Fg }
}

fn modes_node(pad: Margin, gap: f32, rd: Rounding, labels: bool) -> Node {
    Node::Modes {
        size: Size::Auto,
        gap,
        pad,
        fill: fill(Role::BgAlt),
        round: rd,
        sel_fill: Fill(Role::Sel, 1.0),
        text: Role::Fg,
        sel_text: Role::OnSel,
        labels,
    }
}

fn status_node() -> Node {
    Node::Message {
        size: Size::Auto,
        kind: MsgKind::Status,
        pad: m(10.0),
        fill: fill(Role::BgAlt),
        round: r(10.0),
        text: Role::Fg,
        center: true,
    }
}

fn msg_node(kind: MsgKind, f: Role, text: Role) -> Node {
    Node::Message { size: Size::Auto, kind, pad: m(12.0), fill: fill(f), round: r(PILL), text, center: true }
}

fn tile_el(pad: Margin, rd: Rounding, icon: f32) -> ElStyle {
    ElStyle {
        vertical: true,
        pad,
        round: rd,
        icon,
        gap: 8.0,
        fill: fill(Role::BgAlt),
        sel_fill: fill(Role::Sel),
        text: Role::Fg,
        sel_text: Role::OnSel,
        label: true,
        font: 0.85,
    }
}

fn pad4(l: f32, t: f32, r_: f32, b: f32) -> Margin {
    Margin { left: l, right: r_, top: t, bottom: b }
}

/// Arch window: picture fills the dome, apps sit in the flat lower part.
fn arch_launcher(k: f32) -> Layout {
    let root = stack(
        Dir::Column,
        Size::Auto,
        m(0.0),
        0.0,
        vec![
            with_image(
                stack(
                    Dir::Column,
                    Size::Fixed(290.0 * k),
                    pad4(24.0, 0.0, 24.0, 16.0),
                    0.0,
                    vec![Node::Spacer(Size::Expand(1.0)), search_node(m(14.0), fill(Role::BgAlt), r(12.0), Size::Auto)],
                ),
                0.0,
                Rounding::ZERO,
            ),
            stack(
                Dir::Column,
                Size::Auto,
                pad4(20.0, 14.0, 20.0, 22.0),
                10.0,
                vec![
                    status_node(),
                    Node::List {
                        size: Size::Auto,
                        cols: 1,
                        lines: 6,
                        gap: 6.0,
                        el: row_el(mxy(10.0, 6.0), r(10.0), 30.0, Role::Sel, Role::OnSel),
                    },
                    modes_node(mxy(6.0, 10.0), 10.0, r(10.0), true),
                ],
            ),
        ],
    );
    Layout { id: "arch", name: "Arch", blurb: "", width: 520.0 * k, root, outline: Outline::Arch, height: None }
}

fn arch_power(k: f32) -> Layout {
    let root = stack(
        Dir::Column,
        Size::Auto,
        m(0.0),
        0.0,
        vec![
            with_image(
                stack(
                    Dir::Column,
                    Size::Fixed(250.0 * k),
                    pad4(40.0, 0.0, 40.0, 16.0),
                    0.0,
                    vec![Node::Spacer(Size::Expand(1.0)), msg_node(MsgKind::Header, Role::Urg, Role::Fg)],
                ),
                0.0,
                Rounding::ZERO,
            ),
            stack(
                Dir::Column,
                Size::Auto,
                pad4(24.0, 16.0, 24.0, 24.0),
                16.0,
                vec![
                    Node::List { size: Size::Auto, cols: 3, lines: 2, gap: 14.0, el: tile_el(mxy(6.0, 16.0), r(14.0), 30.0) },
                    msg_node(MsgKind::Info, Role::Act, Role::Bg),
                ],
            ),
        ],
    );
    Layout { id: "arch", name: "Arch", blurb: "", width: 460.0 * k, root, outline: Outline::Arch, height: None }
}

/// Everything inside a circle/polygon: picture in the top, list across the middle.
fn inside_launcher(sides: u8, k: f32) -> Layout {
    // Fewer sides = less usable room, so make the window a little bigger.
    let size = 640.0 * k / inradius_factor(sides).powf(0.7);
    let root = stack(
        Dir::Column,
        Size::Auto,
        m(0.0),
        0.0,
        vec![
            with_image(
                stack(
                    Dir::Column,
                    Size::Fixed(size * 0.35),
                    pad4(60.0, 0.0, 60.0, 12.0),
                    0.0,
                    vec![Node::Spacer(Size::Expand(1.0)), search_node(mxy(16.0, 11.0), fill(Role::BgAlt), r(PILL), Size::Auto)],
                ),
                0.0,
                Rounding::ZERO,
            ),
            stack(
                Dir::Column,
                Size::Expand(1.0),
                pad4(36.0, 12.0, 36.0, 34.0),
                8.0,
                vec![
                    Node::List {
                        size: Size::Auto,
                        cols: 1,
                        lines: 5,
                        gap: 5.0,
                        el: row_el(mxy(12.0, 5.0), r(PILL), 28.0, Role::Sel, Role::OnSel),
                    },
                    Node::Spacer(Size::Expand(1.0)),
                    modes_node(m(12.0), 12.0, r(PILL), false),
                ],
            ),
        ],
    );
    Layout { id: "inside", name: "Inside", blurb: "", width: size, root, outline: Outline::Polygon(sides), height: Some(size) }
}

fn inside_power(sides: u8, k: f32) -> Layout {
    let size = 560.0 * k / inradius_factor(sides).powf(0.7);
    let root = with_image(
        stack(
            Dir::Column,
            Size::Auto,
            pad4(70.0, 64.0, 70.0, 64.0),
            14.0,
            vec![
                msg_node(MsgKind::Header, Role::Urg, Role::Fg),
                Node::Spacer(Size::Expand(1.0)),
                Node::List { size: Size::Auto, cols: 3, lines: 2, gap: 16.0, el: tile_el(mxy(6.0, 14.0), r(PILL), 30.0) },
                Node::Spacer(Size::Expand(1.0)),
                msg_node(MsgKind::Info, Role::Act, Role::Bg),
            ],
        ),
        0.5,
        Rounding::ZERO,
    );
    Layout { id: "inside", name: "Inside", blurb: "", width: size, root, outline: Outline::Polygon(sides), height: Some(size) }
}

// ---- Rings ----------------------------------------------------------------

const RING_GAP: f32 = 16.0;
const RING_BAND: f32 = 86.0;
const RING_OUTER_GAP: f32 = 34.0;
const RING_MARGIN: f32 = 34.0;

/// Picture radius used for rings layouts.
fn ring_radius(power: bool) -> f32 {
    if power {
        120.0
    } else {
        150.0
    }
}

/// Distances (circumradius, unscaled) of the ring band centre line and outer mode ring.
fn ring_radii(shape: u8, radius: f32, power: bool) -> (f32, Option<f32>, f32) {
    let k = inradius_factor(shape);
    let band = radius + (RING_GAP + RING_BAND / 2.0) / k;
    let outer = (!power).then(|| band + (RING_BAND / 2.0 + RING_OUTER_GAP) / k);
    let extent = outer.unwrap_or(band + RING_BAND / 2.0 / k) + RING_MARGIN;
    (band, outer, extent)
}

/// How many results fit on the ring.
pub fn ring_capacity(shape: u8, radius: f32, power: bool) -> usize {
    let (band, _, _) = ring_radii(shape, radius, power);
    let perim = poly_perimeter(shape, band);
    if power {
        6
    } else {
        ((perim / 96.0).floor() as usize).clamp(6, 14)
    }
}

fn poly_perimeter(shape: u8, rad: f32) -> f32 {
    if shape < 3 {
        2.0 * std::f32::consts::PI * rad
    } else {
        let n = shape as f32;
        2.0 * n * rad * (std::f32::consts::PI / n).sin()
    }
}

fn rings_layout(shape: u8, power: bool, k: f32) -> Layout {
    let radius = ring_radius(power) * k;
    let (_, _, extent) = ring_radii(shape, radius, power);
    let size = extent * 2.0;
    Layout {
        id: "rings",
        name: "Rings",
        blurb: "",
        width: size,
        root: Node::Rings { shape, radius, power },
        outline: Outline::Rect,
        height: Some(size),
    }
}

// ---- Box beside the picture --------------------------------------------------

const PANEL_CONTENT_W: f32 = 470.0;

fn panel_layout(shape: u8, power: bool, k: f32) -> Layout {
    let radius = if power { 150.0 } else { 175.0 } * k;
    let child = if power {
        stack(
            Dir::Column,
            Size::Auto,
            m(0.0),
            14.0,
            vec![
                msg_node(MsgKind::Header, Role::Urg, Role::Fg),
                Node::List { size: Size::Auto, cols: 3, lines: 2, gap: 12.0, el: tile_el(mxy(6.0, 14.0), r(12.0), 30.0) },
                msg_node(MsgKind::Info, Role::Act, Role::Bg),
            ],
        )
    } else {
        stack(
            Dir::Column,
            Size::Auto,
            m(0.0),
            12.0,
            vec![
                search_node(m(14.0), fill(Role::BgAlt), r(12.0), Size::Auto),
                status_node(),
                Node::List {
                    size: Size::Auto,
                    cols: 1,
                    lines: 7,
                    gap: 6.0,
                    el: row_el(mxy(10.0, 6.0), r(10.0), 30.0, Role::Sel, Role::OnSel),
                },
                modes_node(mxy(6.0, 10.0), 10.0, r(10.0), true),
            ],
        )
    };
    let width = 2.0 * radius + 16.0 + 26.0 + PANEL_CONTENT_W + 24.0;
    Layout {
        id: "box",
        name: "Box",
        blurb: "",
        width,
        root: Node::Panel { shape, radius, child: Box::new(child) },
        outline: Outline::Rect,
        height: None,
    }
}

/// The layout to use for the user's look settings.
pub fn resolve(look: &crate::config::LookConfig, power: bool) -> Layout {
    use crate::config::{MenuStyle, WindowShape};
    let k = look.shape_size.clamp(0.6, 1.6);
    let sides = match look.shape {
        WindowShape::Polygon => look.sides.clamp(3, 12) as u8,
        _ => 0,
    };
    match look.shape {
        WindowShape::Rectangle => {
            if power {
                power_layout(&look.power_layout)
            } else {
                launcher_layout(&look.launcher_layout)
            }
        }
        WindowShape::Arch => {
            if power {
                arch_power(k)
            } else {
                arch_launcher(k)
            }
        }
        WindowShape::Circle | WindowShape::Polygon => match look.menu_style {
            MenuStyle::Rings => rings_layout(sides, power, k),
            MenuStyle::Box => panel_layout(sides, power, k),
            MenuStyle::Inside => {
                if power {
                    inside_power(sides, k)
                } else {
                    inside_launcher(sides, k)
                }
            }
        },
    }
}

impl Layout {
    /// Polygons (in a window of `size` pixels) that make up the visible popup, for cutting the
    /// window to shape when blur is on. None = the plain rectangle.
    pub fn region(&self, size: Vec2) -> Option<Vec<Vec<Pos2>>> {
        let rect = Rect::from_min_size(Pos2::ZERO, size);
        let s = size.x / self.width;
        if self.outline != Outline::Rect {
            return Some(vec![outline_points(self.outline, rect)]);
        }
        match &self.root {
            Node::Rings { .. } => Some(vec![poly_path(0, rect.center(), size.x.min(size.y) / 2.0)]),
            Node::Panel { shape, radius, .. } => {
                let g = panel_geo(rect, *shape, *radius, s);
                let badge = poly_path(*shape, g.c, g.r0 + 12.0 * s);
                let p = g.panel;
                Some(vec![badge, vec![p.left_top(), p.right_top(), p.right_bottom(), p.left_bottom()]])
            }
            _ => None,
        }
    }
}

pub struct PanelGeo {
    pub c: Pos2,
    pub r0: f32,
    pub panel: Rect,
    pub content: Rect,
}

pub fn panel_geo(rect: Rect, shape: u8, radius: f32, s: f32) -> PanelGeo {
    let r0 = radius * s;
    let c = Pos2::new(rect.left() + r0 + 8.0 * s, rect.center().y);
    let panel = Rect::from_min_max(Pos2::new(c.x, rect.top()), rect.max);
    let _ = shape;
    let content = Rect::from_min_max(
        Pos2::new(c.x + r0 + 26.0 * s, panel.top() + 22.0 * s),
        Pos2::new(panel.right() - 24.0 * s, panel.bottom() - 22.0 * s),
    );
    PanelGeo { c, r0, panel, content }
}

/// Points around the window outline, for a window occupying `r`.
pub fn outline_points(o: Outline, r: Rect) -> Vec<Pos2> {
    use std::f32::consts::PI;
    match o {
        Outline::Rect => vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()],
        Outline::Polygon(sides) => poly_path(sides, r.center(), r.width().min(r.height()) / 2.0),
        Outline::Arch => {
            let half = r.width() / 2.0;
            let cy = r.top() + half;
            let mut pts = vec![Pos2::new(r.left(), r.bottom()), Pos2::new(r.right(), r.bottom()), Pos2::new(r.right(), cy)];
            for i in 1..48 {
                let a = i as f32 / 48.0 * PI;
                pts.push(Pos2::new(r.center().x + half * a.cos(), cy - half * a.sin()));
            }
            pts.push(Pos2::new(r.left(), cy));
            pts
        }
    }
}

/// `n` evenly spaced points along a closed path, starting nearest the top centre, going clockwise.
fn resample(path: &[Pos2], n: usize, c: Pos2) -> Vec<Pos2> {
    let m = path.len();
    if m < 2 || n == 0 {
        return vec![c; n];
    }
    let mut lens = Vec::with_capacity(m);
    let mut total = 0.0;
    for i in 0..m {
        let d = path[i].distance(path[(i + 1) % m]);
        lens.push(d);
        total += d;
    }
    // Arc-length position of the point straight above the centre.
    let top = path
        .iter()
        .enumerate()
        .filter(|(_, p)| p.y < c.y)
        .min_by(|a, b| (a.1.x - c.x).abs().partial_cmp(&(b.1.x - c.x).abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0);
    let mut start = 0.0;
    for l in lens.iter().take(top) {
        start += l;
    }
    // For polygons, centre the start on the top edge / vertex.
    let top_y = path.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let flat: Vec<usize> = (0..m).filter(|i| (path[*i].y - top_y).abs() < 0.5).collect();
    if flat.len() == 2 {
        let (a, b) = (flat[0], flat[1]);
        let (first, _) = if path[a].x < path[b].x { (a, b) } else { (b, a) };
        start = lens.iter().take(first).sum::<f32>() + lens[first] / 2.0;
    }
    let point_at = |mut d: f32| {
        d = d.rem_euclid(total);
        for i in 0..m {
            if d <= lens[i] {
                let t = if lens[i] > 0.0 { d / lens[i] } else { 0.0 };
                return path[i] + (path[(i + 1) % m] - path[i]) * t;
            }
            d -= lens[i];
        }
        path[0]
    };
    // Path points go clockwise on screen (y down) for our generators.
    (0..n).map(|k| point_at(start + total * k as f32 / n as f32)).collect()
}

/// Shrink `r` sideways so it fits inside the convex polygon `poly` (with margin `m`).
fn fit_poly(poly: &[Pos2], r: Rect, m: f32) -> Rect {
    let a = span_at(poly, r.top() + 1.0);
    let b = span_at(poly, r.bottom() - 1.0);
    let (Some(a), Some(b)) = (a, b) else { return Rect::NOTHING };
    let left = r.left().max(a.0.max(b.0) + m);
    let right = r.right().min(a.1.min(b.1) - m);
    if right <= left {
        return Rect::from_min_max(Pos2::new(left, r.top()), Pos2::new(left, r.bottom()));
    }
    Rect::from_min_max(Pos2::new(left, r.top()), Pos2::new(right, r.bottom()))
}

/// Left/right x of a convex polygon along the horizontal line at `y`.
fn span_at(poly: &[Pos2], y: f32) -> Option<(f32, f32)> {
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        if (a.y <= y && b.y >= y) || (b.y <= y && a.y >= y) {
            let x = if (b.y - a.y).abs() < 1e-4 { a.x.min(b.x) } else { a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x) };
            let x2 = if (b.y - a.y).abs() < 1e-4 { a.x.max(b.x) } else { x };
            lo = lo.min(x);
            hi = hi.max(x2);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// Clip a convex polygon to a rectangle (Sutherland–Hodgman).
fn clip_to_rect(poly: &[Pos2], r: Rect) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = poly.to_vec();
    // (inside test, intersection) for each of the four edges.
    let edges: [(&dyn Fn(Pos2) -> bool, &dyn Fn(Pos2, Pos2) -> Pos2); 4] = [
        (&|p: Pos2| p.x >= r.left(), &|a: Pos2, b: Pos2| {
            let t = (r.left() - a.x) / (b.x - a.x);
            a + (b - a) * t
        }),
        (&|p: Pos2| p.x <= r.right(), &|a: Pos2, b: Pos2| {
            let t = (r.right() - a.x) / (b.x - a.x);
            a + (b - a) * t
        }),
        (&|p: Pos2| p.y >= r.top(), &|a: Pos2, b: Pos2| {
            let t = (r.top() - a.y) / (b.y - a.y);
            a + (b - a) * t
        }),
        (&|p: Pos2| p.y <= r.bottom(), &|a: Pos2, b: Pos2| {
            let t = (r.bottom() - a.y) / (b.y - a.y);
            a + (b - a) * t
        }),
    ];
    for (inside, cut) in edges.iter() {
        if out.is_empty() {
            break;
        }
        let input = std::mem::take(&mut out);
        let n = input.len();
        for i in 0..n {
            let cur = input[i];
            let prev = input[(i + n - 1) % n];
            match (inside(cur), inside(prev)) {
                (true, true) => out.push(cur),
                (true, false) => {
                    out.push(cut(prev, cur));
                    out.push(cur);
                }
                (false, true) => out.push(cut(prev, cur)),
                (false, false) => {}
            }
        }
    }
    out
}

pub fn launcher_layout(id: &str) -> Layout {
    let mut all = launcher_layouts();
    let i = all.iter().position(|l| l.id == id).unwrap_or(0);
    all.swap_remove(i)
}

pub fn power_layout(id: &str) -> Layout {
    let mut all = power_layouts();
    let i = all.iter().position(|l| l.id == id).unwrap_or(0);
    all.swap_remove(i)
}

// ---------------------------------------------------------------------------
// Content that gets drawn into a layout
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub sub: String,
    pub icon: IconSrc,
}

#[derive(Clone, Copy, Debug)]
pub struct ModeBtn {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
}

pub const MODES: &[ModeBtn] = &[
    ModeBtn { id: "apps", label: "Apps", icon: "apps" },
    ModeBtn { id: "windows", label: "Windows", icon: "windows" },
    ModeBtn { id: "run", label: "Run", icon: "run" },
    ModeBtn { id: "power", label: "Power", icon: "power" },
];

/// Look options that come straight from the user's settings.
#[derive(Clone, Copy, Debug)]
pub struct LookOpts {
    /// Multiplies every corner radius.
    pub roundness: f32,
    pub image_zoom: f32,
    pub image_focus: [f32; 2],
    pub picture_shape: PictureShape,
    pub button_shape: ButtonShape,
    /// Opacity of boxes/buttons sitting on top of the picture.
    pub overlay: f32,
    /// Darkening for layouts that use the picture as a full background.
    pub picture_dim: f32,
}

impl LookOpts {
    pub fn from(look: &LookConfig) -> Self {
        LookOpts {
            roundness: look.roundness,
            image_zoom: look.image_zoom.max(1.0),
            image_focus: look.image_focus,
            picture_shape: look.picture_shape,
            button_shape: look.button_shape,
            overlay: look.overlay_opacity.clamp(0.0, 1.0),
            picture_dim: look.picture_dim.clamp(0.0, 0.95),
        }
    }
}

/// Everything the renderer needs for one frame.
pub struct Scene<'a> {
    pub colors: Colors,
    /// Multiplies every length (user "size" setting, or a small value for thumbnails).
    pub scale: f32,
    pub opts: LookOpts,
    /// Fade-in amount, 0..1.
    pub alpha: f32,
    pub base_font: f32,
    /// Physical pixels per point (for crisp icons).
    pub ppp: f32,
    pub image: Option<&'a egui::TextureHandle>,
    pub items: &'a [Item],
    pub selected: usize,
    pub first_row: usize,
    pub mode: usize,
    pub header: &'a str,
    pub info: &'a str,
    pub status: &'a str,
    /// Some = live, editable search box. None = draw the text only (previews).
    pub query: Option<&'a mut String>,
    pub query_preview: &'a str,
    pub focus_search: bool,
    pub interactive: bool,
    /// Let the user drag/scroll the picture to frame it (settings preview).
    pub edit_picture: bool,
    pub icons: &'a mut IconCache,
    /// Unique per drawn instance so egui ids don't clash in the gallery.
    pub id_salt: &'a str,
    /// Window outline for shaped layouts (filled in by `draw`; leave empty).
    pub clip: Vec<Pos2>,
}

#[derive(Default, Debug)]
pub struct Output {
    pub clicked: Option<usize>,
    pub context: Option<(usize, Pos2)>,
    pub mode_clicked: Option<usize>,
    /// +1 / -1 rows from the mouse wheel.
    pub scroll: i32,
    pub first_row: usize,
    /// Picture framing changes (in picture coordinates, 0..1) from dragging.
    pub pan: Vec2,
    /// Multiplier from scrolling over the picture (1.0 = unchanged).
    pub zoom: f32,
    /// The part of the picture currently shown in the editable picture box.
    pub picture_uv: Option<Rect>,
}

/// Which part of a picture to show in a box of aspect `box_aspect`, honouring zoom and focus.
pub fn cover_uv(tex: Vec2, box_aspect: f32, zoom: f32, focus: [f32; 2]) -> Rect {
    let ta = tex.x / tex.y.max(1.0);
    let (mut w, mut h) = if ta > box_aspect { (box_aspect / ta, 1.0) } else { (1.0, ta / box_aspect) };
    w /= zoom.max(1.0);
    h /= zoom.max(1.0);
    let cx = focus[0].clamp(w / 2.0, 1.0 - w / 2.0);
    let cy = focus[1].clamp(h / 2.0, 1.0 - h / 2.0);
    Rect::from_center_size(Pos2::new(cx, cy), Vec2::new(w, h))
}

/// Outline points for a picture frame shape inside `area`, plus the shape's bounding box.
fn picture_points(shape: PictureShape, area: Rect) -> (Vec<Pos2>, Rect) {
    use std::f32::consts::PI;
    let side = area.width().min(area.height());
    let sq = Rect::from_center_size(area.center(), Vec2::splat(side));
    let c = sq.center();
    let rad = side / 2.0;
    match shape {
        PictureShape::Circle | PictureShape::Fill => {
            let pts = (0..72).map(|i| {
                let a = i as f32 / 72.0 * 2.0 * PI;
                Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())
            });
            (pts.collect(), sq)
        }
        PictureShape::Squircle => {
            let pts = (0..96).map(|i| {
                let a = i as f32 / 96.0 * 2.0 * PI;
                let (ca, sa) = (a.cos(), a.sin());
                let x = ca.signum() * ca.abs().powf(0.5);
                let y = sa.signum() * sa.abs().powf(0.5);
                Pos2::new(c.x + rad * x, c.y + rad * y)
            });
            (pts.collect(), sq)
        }
        PictureShape::Hexagon => {
            let pts = (0..6).map(|i| {
                let a = (-90.0 + 60.0 * i as f32).to_radians();
                Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())
            });
            let pts: Vec<Pos2> = pts.collect();
            (pts, sq)
        }
        PictureShape::Diamond => {
            let pts = vec![
                Pos2::new(c.x, sq.top()),
                Pos2::new(sq.right(), c.y),
                Pos2::new(c.x, sq.bottom()),
                Pos2::new(sq.left(), c.y),
            ];
            (pts, sq)
        }
        PictureShape::Arch => {
            let w = area.width().min(area.height() * 0.78);
            let r = Rect::from_center_size(area.center(), Vec2::new(w, area.height()));
            let half = w / 2.0;
            let cy = r.top() + half;
            let mut pts = vec![Pos2::new(r.left(), r.bottom()), Pos2::new(r.right(), r.bottom()), Pos2::new(r.right(), cy)];
            for i in 1..36 {
                let a = i as f32 / 36.0 * PI;
                pts.push(Pos2::new(r.center().x + half * a.cos(), cy - half * a.sin()));
            }
            pts.push(Pos2::new(r.left(), cy));
            (pts, r)
        }
    }
}

impl<'a> Scene<'a> {
    fn c(&self, f: Fill) -> Color32 {
        self.colors.get(f.0).gamma_multiply(f.1 * self.alpha)
    }
    /// Colour for a box that may sit on top of the picture.
    fn c_on(&self, f: Fill, on_image: bool) -> Color32 {
        if on_image {
            self.c(Fill(f.0, f.1 * self.opts.overlay))
        } else {
            self.c(f)
        }
    }
    fn role(&self, r: Role) -> Color32 {
        self.colors.get(r).gamma_multiply(self.alpha)
    }
    fn font(&self, k: f32) -> FontId {
        FontId::new(self.base_font * k * self.scale, FontFamily::Proportional)
    }
    fn bold(&self, k: f32) -> FontId {
        FontId::new(self.base_font * k * self.scale, FontFamily::Name("bold".into()))
    }
    fn text_h(&self, k: f32) -> f32 {
        self.base_font * k * self.scale * 1.3
    }
    fn mg(&self, mg: Margin) -> Margin {
        let s = self.scale;
        Margin { left: mg.left * s, right: mg.right * s, top: mg.top * s, bottom: mg.bottom * s }
    }
    fn rd(&self, rd: Rounding) -> Rounding {
        let k = self.scale * self.opts.roundness;
        Rounding { nw: rd.nw * k, ne: rd.ne * k, sw: rd.sw * k, se: rd.se * k }
    }
    fn msg_text(&self, kind: MsgKind) -> &str {
        match kind {
            MsgKind::Header => self.header,
            MsgKind::Info => self.info,
            MsgKind::Status => self.status,
        }
    }

    /// Fill a box in the user's chosen button shape.
    fn shape_fill(&self, ui: &Ui, rect: Rect, rd: Rounding, color: Color32) {
        if color.a() == 0 {
            return;
        }
        let p = ui.painter();
        let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
        let h = rect.height();
        match self.opts.button_shape {
            ButtonShape::Theme => {
                p.rect_filled(rect, rd, color);
            }
            ButtonShape::Square => {
                p.rect_filled(rect, Rounding::ZERO, color);
            }
            ButtonShape::Pill | ButtonShape::Circle => {
                p.rect_filled(rect, Rounding::same(h.min(rect.width()) / 2.0), color);
            }
            ButtonShape::Slanted => {
                let k = (h * 0.3).min(rect.width() * 0.2);
                let pts = vec![Pos2::new(l + k, t), Pos2::new(r, t), Pos2::new(r - k, b), Pos2::new(l, b)];
                p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
            }
            ButtonShape::Hexagon => {
                let k = (h * 0.5).min(rect.width() * 0.25);
                let cy = rect.center().y;
                let pts = vec![
                    Pos2::new(l + k, t),
                    Pos2::new(r - k, t),
                    Pos2::new(r, cy),
                    Pos2::new(r - k, b),
                    Pos2::new(l + k, b),
                    Pos2::new(l, cy),
                ];
                p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
            }
        }
    }

    /// Extra side padding so content doesn't poke out of slanted/hexagon ends.
    fn shape_inset(&self, h: f32) -> f32 {
        match self.opts.button_shape {
            ButtonShape::Slanted => h * 0.2,
            ButtonShape::Hexagon => h * 0.3,
            ButtonShape::Pill | ButtonShape::Circle => h * 0.15,
            _ => 0.0,
        }
    }

    /// Paint text, with a soft shadow when it sits on the picture so it stays readable.
    fn text(&self, ui: &Ui, pos: Pos2, galley: std::sync::Arc<egui::Galley>, color: Color32, on_image: bool) {
        if on_image {
            let shadow = Color32::from_black_alpha((150.0 * self.alpha) as u8);
            ui.painter().galley_with_override_text_color(pos + Vec2::new(1.0, 1.0), galley.clone(), shadow);
        }
        ui.painter().galley(pos, galley, color);
    }

    fn row_height(&self, el: &ElStyle) -> f32 {
        let pad = self.mg(el.pad);
        let icon = el.icon * self.scale;
        let th = self.text_h(el.font);
        if el.vertical {
            icon + if el.label { el.gap * self.scale + th } else { 0.0 } + pad.top + pad.bottom
        } else {
            icon.max(th) + pad.top + pad.bottom
        }
    }

    /// Natural length of `n` along an axis.
    fn measure(&self, n: &Node, vertical: bool) -> f32 {
        let s = self.scale;
        match n {
            Node::Spacer(_) => 0.0,
            Node::Rings { shape, radius, power } => 2.0 * ring_radii(*shape, *radius, *power).2 * s,
            Node::Panel { radius, child, .. } => {
                if vertical {
                    ((2.0 * radius + 16.0) * s).max(self.measure(child, true) + 44.0 * s)
                } else {
                    (2.0 * radius + 16.0 + 26.0 + PANEL_CONTENT_W + 24.0) * s
                }
            }
            Node::Search { pad, .. } => {
                let p = self.mg(*pad);
                if vertical {
                    self.text_h(1.0) + p.top + p.bottom
                } else {
                    220.0 * s
                }
            }
            Node::Modes { pad, gap, labels, .. } => {
                let p = self.mg(*pad);
                let icon = 20.0 * s;
                if vertical {
                    icon.max(self.text_h(0.95)) + p.top + p.bottom
                } else {
                    let w = if *labels { 100.0 * s } else { icon + p.left + p.right };
                    w * MODES.len() as f32 + gap * s * (MODES.len() - 1) as f32
                }
            }
            Node::Message { pad, kind, .. } => {
                if self.msg_text(*kind).is_empty() {
                    return 0.0;
                }
                let p = self.mg(*pad);
                if vertical {
                    self.text_h(1.0) + p.top + p.bottom
                } else {
                    200.0 * s
                }
            }
            Node::List { lines, gap, el, cols, .. } => {
                if vertical {
                    let rh = self.row_height(el);
                    rh * *lines as f32 + gap * s * (lines.saturating_sub(1)) as f32
                } else {
                    let pad = self.mg(el.pad);
                    (el.icon * s + pad.left + pad.right + 60.0 * s) * *cols as f32
                }
            }
            Node::Stack { dir, pad, gap, children, .. } => {
                let p = self.mg(*pad);
                let along = (*dir == Dir::Column) == vertical;
                let sizes: Vec<f32> = children
                    .iter()
                    .map(|c| match (c.size(), along) {
                        (Size::Fixed(v), true) => v * s,
                        _ => self.measure(c, vertical),
                    })
                    .collect();
                let inner = if along {
                    let visible = sizes.iter().filter(|v| **v > 0.0).count();
                    sizes.iter().sum::<f32>() + gap * s * visible.saturating_sub(1) as f32
                } else {
                    sizes.iter().cloned().fold(0.0, f32::max)
                };
                inner + if vertical { p.top + p.bottom } else { p.left + p.right }
            }
        }
    }

    /// Size (in points) the whole layout wants.
    pub fn layout_size(&self, layout: &Layout) -> Vec2 {
        let h = layout.height.map(|h| h * self.scale).unwrap_or_else(|| self.measure(&layout.root, true));
        Vec2::new(layout.width * self.scale, h)
    }

    /// Draw `layout` with its top-left corner at `origin`.
    pub fn draw(&mut self, ui: &mut Ui, layout: &Layout, origin: Pos2) -> Output {
        let size = self.layout_size(layout);
        let rect = Rect::from_min_size(origin, size);
        let mut out = Output { first_row: self.first_row, zoom: 1.0, ..Default::default() };
        if layout.outline == Outline::Rect {
            self.clip.clear();
        } else {
            // Shaped window: paint the silhouette; content is fitted inside it.
            self.clip = outline_points(layout.outline, rect);
            let bg = self.c(Fill(Role::Bg, 1.0));
            ui.painter().add(egui::Shape::convex_polygon(self.clip.clone(), bg, egui::Stroke::NONE));
        }
        self.node(ui, &layout.root, rect, &mut out, false);
        out
    }

    /// Shrink a box sideways so it stays inside a shaped window.
    fn fit(&self, r: Rect) -> Rect {
        if self.clip.is_empty() {
            return r;
        }
        fit_poly(&self.clip, r, 6.0 * self.scale)
    }

    /// Work out where each child of a stack goes. None = child takes no space.
    fn arrange(&self, dir: Dir, inner: Rect, gap: f32, children: &[Node]) -> Vec<Option<Rect>> {
        let vertical = dir == Dir::Column;
        let main_len = if vertical { inner.height() } else { inner.width() };
        let gap = gap * self.scale;
        let mut lens: Vec<f32> = Vec::with_capacity(children.len());
        let mut weights = 0.0;
        for c in children {
            match c.size() {
                Size::Fixed(v) => lens.push(v * self.scale),
                Size::Auto => lens.push(self.measure(c, vertical)),
                Size::Expand(w) => {
                    weights += w;
                    lens.push(0.0)
                }
            }
        }
        let shown = |c: &Node, l: f32| l > 0.0 || matches!(c.size(), Size::Expand(_));
        let visible = children.iter().zip(&lens).filter(|(c, l)| shown(c, **l)).count();
        let used: f32 = lens.iter().sum::<f32>() + gap * visible.saturating_sub(1) as f32;
        let left = (main_len - used).max(0.0);
        for (c, l) in children.iter().zip(lens.iter_mut()) {
            if let Size::Expand(w) = c.size() {
                *l = if weights > 0.0 { left * w / weights } else { 0.0 };
            }
        }
        let mut pos = if vertical { inner.top() } else { inner.left() };
        let mut out = Vec::with_capacity(children.len());
        for (c, l) in children.iter().zip(lens.iter()) {
            if !shown(c, *l) {
                out.push(None);
                continue;
            }
            let r = if vertical {
                Rect::from_min_size(Pos2::new(inner.left(), pos), Vec2::new(inner.width(), *l))
            } else {
                Rect::from_min_size(Pos2::new(pos, inner.top()), Vec2::new(*l, inner.height()))
            };
            out.push(Some(r));
            pos += l + gap;
        }
        out
    }

    fn node(&mut self, ui: &mut Ui, n: &Node, rect: Rect, out: &mut Output, on_image: bool) {
        match n {
            Node::Spacer(_) => {}
            Node::Rings { shape, radius, power } => self.draw_rings(ui, rect, *shape, *radius, *power, out),
            Node::Panel { shape, radius, child } => self.draw_panel(ui, rect, *shape, *radius, child, out),
            Node::Stack { dir, pad, gap, fill, round, image, children, .. } => {
                let rd = self.rd(*round);
                if let Some(f) = fill {
                    ui.painter().rect_filled(rect, rd, self.c(*f));
                }
                let inner = rect - self.mg(*pad);
                let rects = self.arrange(*dir, inner, *gap, children);
                let mut child_on_image = on_image;
                if let Some(preset_dim) = image {
                    if *preset_dim > 0.0 {
                        // Layouts that use the picture as a full background.
                        let dim = self.opts.picture_dim;
                        self.draw_image(ui, rect, rd, dim, out);
                        child_on_image = dim < 0.45;
                    } else if self.opts.picture_shape == PictureShape::Fill {
                        self.draw_image(ui, rect, rd, 0.0, out);
                        child_on_image = true;
                    } else {
                        // Framed picture: put it in the biggest empty gap between the
                        // stack's children if there's a decent one, else the whole box.
                        let free = children
                            .iter()
                            .zip(&rects)
                            .filter(|(c, _)| matches!(c, Node::Spacer(_)))
                            .filter_map(|(_, r)| *r)
                            .max_by(|a, b| a.area().partial_cmp(&b.area()).unwrap());
                        let min_side = |r: Rect| r.width().min(r.height());
                        let area = match free {
                            Some(f) if min_side(f) >= 0.45 * min_side(inner) => f.shrink(8.0 * self.scale),
                            _ => inner,
                        };
                        self.draw_framed_image(ui, area, out);
                        child_on_image = false;
                    }
                }
                for (c, r) in children.iter().zip(rects) {
                    if let Some(r) = r {
                        self.node(ui, c, r, out, child_on_image);
                    }
                }
            }
            Node::Search { pad, fill, round, text, .. } => self.draw_search(ui, rect, *pad, *fill, *round, *text, on_image),
            Node::Modes { gap, pad, fill, round, sel_fill, text, sel_text, labels, .. } => {
                self.draw_modes(ui, rect, *gap, *pad, *fill, *round, *sel_fill, *text, *sel_text, *labels, out, on_image)
            }
            Node::Message { kind, pad, fill, round, text, center, .. } => {
                let msg = self.msg_text(*kind).to_string();
                let rect = self.fit(rect);
                if msg.is_empty() || rect.width() < 30.0 {
                    return;
                }
                if let Some(f) = fill {
                    self.shape_fill(ui, rect, self.rd(*round), self.c_on(*f, on_image));
                }
                let mut inner = rect - self.mg(*pad);
                let inset = self.shape_inset(rect.height());
                inner.min.x += inset;
                inner.max.x -= inset;
                let color = self.role(*text);
                let icon_name = match kind {
                    MsgKind::Header => Some(("ui:user", "user")),
                    MsgKind::Info => Some(("ui:clock", "clock")),
                    MsgKind::Status => None,
                };
                let font = if *kind == MsgKind::Header { self.bold(0.95) } else { self.font(0.95) };
                let galley = text_galley(ui, &msg, font, color, inner.width() - 30.0 * self.scale);
                let icon_w = if icon_name.is_some() { 16.0 * self.scale + 8.0 * self.scale } else { 0.0 };
                let total = galley.size().x + icon_w;
                let x0 = if *center { inner.center().x - total / 2.0 } else { inner.left() };
                if let Some((id, name)) = icon_name {
                    let s = 16.0 * self.scale;
                    let ir = Rect::from_min_size(Pos2::new(x0, inner.center().y - s / 2.0), Vec2::splat(s));
                    self.draw_icon(ui, id, &IconSrc::Builtin(name), ir, color);
                }
                let pos = Pos2::new(x0 + icon_w, inner.center().y - galley.size().y / 2.0);
                self.text(ui, pos, galley, color, on_image);
            }
            Node::List { cols, lines, gap, el, .. } => self.draw_list(ui, rect, *cols, *lines, *gap, el, out),
        }
    }

    /// The user's picture inside a circle/polygon.
    fn picture_in(&mut self, ui: &mut Ui, poly: &[Pos2], c: Pos2, r0: f32, out: &mut Output) {
        let bounds = Rect::from_center_size(c, Vec2::splat(2.0 * r0));
        if let Some(tex) = self.image {
            let ts = tex.size_vec2();
            let uv = cover_uv(ts, 1.0, self.opts.image_zoom, self.opts.image_focus);
            paint_textured(ui, tex.id(), poly, bounds, uv, Color32::WHITE.gamma_multiply(self.alpha));
            self.picture_editing(ui, bounds, uv, out);
        } else {
            let fillc = self.c(Fill(Role::BgAlt, 1.0));
            ui.painter().add(egui::Shape::convex_polygon(poly.to_vec(), fillc, egui::Stroke::NONE));
        }
        let edge = egui::Stroke::new(3.0 * self.scale, self.colors.bg_alt.gamma_multiply(self.alpha));
        ui.painter().add(egui::Shape::closed_line(poly.to_vec(), edge));
    }

    /// Picture in the middle, results on a ring band around it, mode buttons on an outer ring.
    fn draw_rings(&mut self, ui: &mut Ui, rect: Rect, shape: u8, radius: f32, power: bool, out: &mut Output) {
        let s = self.scale;
        let c = rect.center();
        let k = inradius_factor(shape);
        let (band_r, outer_r, _) = ring_radii(shape, radius, power);
        let r0 = radius * s;
        let band_w = RING_BAND * s;

        // The band the results sit on.
        let band_path = poly_path(shape, c, band_r * s);
        let band_col = self.c(Fill(Role::Bg, 1.0));
        ui.painter().add(egui::Shape::closed_line(band_path.clone(), egui::Stroke::new(band_w, band_col)));

        // Picture with the search box (or name / uptime) inside it.
        let pic = poly_path(shape, c, r0);
        self.picture_in(ui, &pic, c, r0, out);
        let th = self.text_h(1.0);
        if !power {
            let h = th + 22.0 * s;
            let want = Rect::from_center_size(Pos2::new(c.x, c.y + r0 * k * 0.58), Vec2::new(r0 * 1.4, h));
            let rr = fit_poly(&pic, want, 10.0 * s);
            let node = search_node(mxy(14.0, 11.0), fill(Role::BgAlt), Rounding::same(PILL), Size::Auto);
            self.node(ui, &node, rr, out, true);
        } else {
            for (kind, f, t, dy) in [(MsgKind::Header, Role::Urg, Role::Fg, -0.55f32), (MsgKind::Info, Role::Act, Role::Bg, 0.55)]
            {
                let want = Rect::from_center_size(Pos2::new(c.x, c.y + r0 * k * dy), Vec2::new(r0 * 1.5, th + 24.0 * s));
                let rr = fit_poly(&pic, want, 10.0 * s);
                self.node(ui, &msg_node(kind, f, t), rr, out, true);
            }
        }

        // Results around the band.
        let items: &'a [Item] = self.items;
        let cap = ring_capacity(shape, radius, power);
        let mut first = self.first_row;
        if self.selected < first {
            first = self.selected;
        }
        if self.selected >= first + cap {
            first = self.selected + 1 - cap;
        }
        first = if items.len() <= cap { 0 } else { first.min(items.len() - cap) };
        out.first_row = first;
        let slots = resample(&band_path, cap, c);
        let spacing = poly_perimeter(shape, band_r * s) / cap as f32;
        let rb = 23.0 * s;
        let icon = 28.0 * s;
        if self.interactive {
            let resp = ui.interact(rect, Id::new(("hestia-ring", self.id_salt)), Sense::hover());
            if resp.hovered() {
                let dy = ui.input(|i| i.raw_scroll_delta.y);
                if dy > 0.5 {
                    out.scroll = -1;
                } else if dy < -0.5 {
                    out.scroll = 1;
                }
            }
        }
        if items.is_empty() {
            let col = self.role(Role::Fg).gamma_multiply(0.7);
            let g = text_galley(ui, "Nothing found", self.font(0.9), col, 200.0 * s);
            let p = slots.first().copied().unwrap_or(c);
            ui.painter().galley(Pos2::new(p.x - g.size().x / 2.0, p.y - g.size().y / 2.0), g, col);
        }
        for (k, p) in slots.iter().enumerate() {
            let idx = first + k;
            let bc = *p - Vec2::new(0.0, 9.0 * s);
            let Some(item) = items.get(idx) else {
                ui.painter().circle_filled(bc, 4.0 * s, self.c(Fill(Role::BgAlt, 0.8)));
                continue;
            };
            let selected = idx == self.selected;
            let hit = Rect::from_center_size(*p, Vec2::new(spacing.min(band_w * 1.2), band_w * 0.95));
            let mut hovered = false;
            if self.interactive {
                let resp = ui.interact(hit, Id::new(("hestia-ring-item", self.id_salt, idx)), Sense::click());
                hovered = resp.hovered();
                if resp.clicked() {
                    out.clicked = Some(idx);
                }
                if resp.secondary_clicked() {
                    out.context = Some((idx, resp.interact_pointer_pos().unwrap_or(*p)));
                }
            }
            let rad = if selected { rb * 1.15 } else { rb };
            let bg = if selected {
                self.c(Fill(Role::Sel, 1.0))
            } else if hovered {
                self.c(Fill(Role::Sel, 0.35))
            } else {
                self.c(Fill(Role::BgAlt, 1.0))
            };
            ui.painter().circle_filled(bc, rad, bg);
            let tint = self.role(if selected { Role::OnSel } else { Role::Fg });
            self.draw_icon(ui, &item.id, &item.icon, Rect::from_center_size(bc, Vec2::splat(icon)), tint);
            let font = if selected { self.bold(0.72) } else { self.font(0.72) };
            let col = self.role(Role::Fg);
            let g = text_galley(ui, &item.label, font, col, (spacing - 8.0 * s).max(20.0));
            ui.painter().galley(Pos2::new(p.x - g.size().x / 2.0, bc.y + rb + 3.0 * s), g, col);
        }

        // Mode buttons on the outer ring (diagonals, so they sit between the corners).
        if let Some(or) = outer_r {
            let path = poly_path(shape, c, or * s);
            let line = egui::Stroke::new(2.0 * s, self.colors.bg_alt.gamma_multiply(0.9 * self.alpha));
            ui.painter().add(egui::Shape::closed_line(path.clone(), line));
            let pts = resample(&path, 8, c);
            for (i, m) in MODES.iter().enumerate() {
                let p = pts[1 + 2 * i];
                let rr = 21.0 * s;
                let selected = i == self.mode;
                let mut hovered = false;
                if self.interactive {
                    let resp = ui.interact(
                        Rect::from_center_size(p, Vec2::splat(2.0 * rr)),
                        Id::new(("hestia-ring-mode", self.id_salt, i)),
                        Sense::click(),
                    );
                    hovered = resp.hovered();
                    if resp.clicked() {
                        out.mode_clicked = Some(i);
                    }
                    resp.on_hover_text(m.label);
                }
                ui.painter().circle_filled(p, rr + 3.0 * s, self.c(Fill(Role::Bg, 1.0)));
                let bg = if selected {
                    self.c(Fill(Role::Sel, 1.0))
                } else if hovered {
                    self.c(Fill(Role::Sel, 0.35))
                } else {
                    self.c(Fill(Role::BgAlt, 1.0))
                };
                ui.painter().circle_filled(p, rr, bg);
                let tint = self.role(if selected { Role::OnSel } else { Role::Fg });
                let id = format!("mode:{}", m.id);
                self.draw_icon(ui, &id, &IconSrc::Builtin(m.icon), Rect::from_center_size(p, Vec2::splat(18.0 * s)), tint);
            }
        }
    }

    /// Picture badge with a flat menu box coming out of its side.
    fn draw_panel(&mut self, ui: &mut Ui, rect: Rect, shape: u8, radius: f32, child: &Node, out: &mut Output) {
        let g = panel_geo(rect, shape, radius, self.scale);
        let rd = self.rd(Rounding::same(18.0));
        ui.painter().rect_filled(g.panel, rd, self.c(Fill(Role::Bg, 1.0)));
        self.node(ui, child, g.content, out, false);
        let border = poly_path(shape, g.c, g.r0 + 8.0 * self.scale);
        let bc = self.c(Fill(Role::Bg, 1.0));
        ui.painter().add(egui::Shape::convex_polygon(border, bc, egui::Stroke::NONE));
        let pic = poly_path(shape, g.c, g.r0);
        self.picture_in(ui, &pic, g.c, g.r0, out);
    }

    /// Picture filling `rect` (cropped to fit), optionally darkened.
    fn draw_image(&mut self, ui: &mut Ui, rect: Rect, rd: Rounding, dim: f32, out: &mut Output) {
        let Some(tex) = self.image else { return };
        let ts = tex.size_vec2();
        if ts.x <= 0.0 || ts.y <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        let uv = cover_uv(ts, rect.width() / rect.height(), self.opts.image_zoom, self.opts.image_focus);
        if !self.clip.is_empty() {
            // Shaped window: only draw the part of the picture inside the outline.
            let poly = clip_to_rect(&self.clip, rect);
            if poly.len() >= 3 {
                let tint = Color32::WHITE.gamma_multiply(self.alpha);
                paint_textured(ui, tex.id(), &poly, rect, uv, tint);
                if dim > 0.0 {
                    let c = self.colors.bg.gamma_multiply(dim * self.alpha);
                    ui.painter().add(egui::Shape::convex_polygon(poly, c, egui::Stroke::NONE));
                }
            }
            self.picture_editing(ui, rect, uv, out);
            return;
        }
        egui::Image::new(egui::load::SizedTexture::new(tex.id(), ts))
            .uv(uv)
            .rounding(rd)
            .tint(Color32::WHITE.gamma_multiply(self.alpha))
            .paint_at(ui, rect);
        if dim > 0.0 {
            ui.painter().rect_filled(rect, rd, self.colors.bg.gamma_multiply(dim * self.alpha));
        }
        self.picture_editing(ui, rect, uv, out);
    }

    /// Picture inside a circle / arch / hexagon... frame.
    fn draw_framed_image(&mut self, ui: &mut Ui, area: Rect, out: &mut Output) {
        let Some(tex) = self.image else { return };
        let ts = tex.size_vec2();
        if ts.x <= 0.0 || ts.y <= 0.0 || area.width() < 4.0 || area.height() < 4.0 {
            return;
        }
        let (pts, bounds) = picture_points(self.opts.picture_shape, area);
        let uv = cover_uv(ts, bounds.width() / bounds.height(), self.opts.image_zoom, self.opts.image_focus);
        let tint = Color32::WHITE.gamma_multiply(self.alpha);
        paint_textured(ui, tex.id(), &pts, bounds, uv, tint);
        // A soft ring hides jagged edges and makes the frame look intentional.
        let ring = egui::Stroke::new(2.5 * self.scale.max(0.5), self.colors.bg_alt.gamma_multiply(self.alpha));
        ui.painter().add(egui::Shape::closed_line(pts, ring));
        self.picture_editing(ui, bounds, uv, out);
    }

    /// In settings, dragging the picture moves it and scrolling zooms it.
    fn picture_editing(&mut self, ui: &mut Ui, rect: Rect, uv: Rect, out: &mut Output) {
        if !self.edit_picture {
            return;
        }
        out.picture_uv = Some(uv);
        let resp = ui.interact(rect, Id::new(("hestia-picture", self.id_salt)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if resp.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
            let dy = ui.input(|i| i.raw_scroll_delta.y);
            if dy.abs() > 0.1 {
                out.zoom *= (1.0 + dy * 0.0015).clamp(0.8, 1.25);
            }
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            out.pan += Vec2::new(-d.x / rect.width() * uv.width(), -d.y / rect.height() * uv.height());
        }
    }

    fn draw_icon(&mut self, ui: &mut Ui, id: &str, src: &IconSrc, rect: Rect, tint: Color32) {
        let px = (rect.width() * self.ppp).round().max(8.0) as u32;
        let icon = self.icons.get(id, src, px).or_else(|| self.icons.placeholder(px));
        if let Some(ic) = icon {
            let t = if ic.tintable { tint } else { Color32::WHITE.gamma_multiply(self.alpha) };
            ui.painter().image(ic.id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), t);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_search(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        pad: Margin,
        fill: Option<Fill>,
        round: Rounding,
        text: Role,
        on_image: bool,
    ) {
        let rect = self.fit(rect);
        if rect.width() < 40.0 * self.scale {
            return;
        }
        if let Some(f) = fill {
            self.shape_fill(ui, rect, self.rd(round), self.c_on(f, on_image));
        }
        let mut inner = rect - self.mg(pad);
        let inset = self.shape_inset(rect.height());
        inner.min.x += inset;
        inner.max.x -= inset;
        let color = self.role(text);
        let icon_s = 18.0 * self.scale;
        let ir = Rect::from_min_size(Pos2::new(inner.left(), inner.center().y - icon_s / 2.0), Vec2::splat(icon_s));
        self.draw_icon(ui, "ui:search", &IconSrc::Builtin("search"), ir, color);
        let th = self.text_h(1.0);
        let tr = Rect::from_min_max(
            Pos2::new(ir.right() + 10.0 * self.scale, inner.center().y - th / 2.0),
            Pos2::new(inner.right(), inner.center().y + th / 2.0),
        );
        let font = self.font(1.0);
        let focus = self.focus_search;
        let salt = self.id_salt.to_string();
        match self.query.as_deref_mut() {
            Some(q) if self.interactive => {
                let te = egui::TextEdit::singleline(q)
                    .id(Id::new(("hestia-search", salt)))
                    .frame(false)
                    .font(font)
                    .text_color(color)
                    .hint_text(RichText::new("Search").color(color.gamma_multiply(0.7)))
                    .margin(Margin::ZERO)
                    .desired_width(tr.width());
                let resp = ui.put(tr, te);
                if focus && !resp.has_focus() {
                    resp.request_focus();
                }
            }
            _ => {
                let (t, c) = if self.query_preview.is_empty() {
                    ("Search", color.gamma_multiply(0.7))
                } else {
                    (self.query_preview, color)
                };
                let g = text_galley(ui, t, font, c, tr.width());
                self.text(ui, Pos2::new(tr.left(), tr.center().y - g.size().y / 2.0), g, c, on_image);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_modes(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        gap: f32,
        pad: Margin,
        fill: Option<Fill>,
        round: Rounding,
        sel_fill: Fill,
        text: Role,
        sel_text: Role,
        labels: bool,
        out: &mut Output,
        on_image: bool,
    ) {
        let rect = self.fit(rect);
        if !(rect.width() > 20.0) {
            return;
        }
        // Round buttons can't hold a label, so they show just the icon (with a tooltip).
        let labels = labels && self.opts.button_shape != ButtonShape::Circle;
        let n = MODES.len() as f32;
        let gap = gap * self.scale;
        let natural = self.measure(
            &Node::Modes { size: Size::Auto, gap: gap / self.scale, pad, fill, round, sel_fill, text, sel_text, labels },
            false,
        );
        // Buttons stretch to fill the row when there's room (type 6), else keep natural width.
        let width = if labels { rect.width() } else { natural.min(rect.width()) };
        let bw = (width - gap * (n - 1.0)) / n;
        let x0 = rect.right() - width;
        for (i, m) in MODES.iter().enumerate() {
            let br = Rect::from_min_size(Pos2::new(x0 + i as f32 * (bw + gap), rect.top()), Vec2::new(bw, rect.height()));
            let selected = i == self.mode;
            let mut hovered = false;
            if self.interactive {
                let resp = ui.interact(br, Id::new(("hestia-mode", self.id_salt, i)), Sense::click());
                hovered = resp.hovered();
                if resp.clicked() {
                    out.mode_clicked = Some(i);
                }
                resp.on_hover_text(m.label);
            }
            let bg = if selected {
                // The chosen tab stays visible even over a picture.
                Some(Fill(sel_fill.0, if on_image { 0.55 + 0.45 * self.opts.overlay } else { 1.0 }))
            } else if hovered {
                Some(Fill(sel_fill.0, 0.35))
            } else {
                fill.map(|f| Fill(f.0, f.1 * if on_image { self.opts.overlay } else { 1.0 }))
            };
            if let Some(f) = bg {
                self.shape_fill(ui, br, self.rd(round), self.c(f));
            }
            let color = self.role(if selected { sel_text } else { text });
            let icon_s = 18.0 * self.scale;
            let item_id = format!("mode:{}", m.id);
            if labels {
                let font = self.bold(0.85);
                let g = text_galley(ui, m.label, font, color, bw - icon_s - 12.0 * self.scale);
                let total = icon_s + 7.0 * self.scale + g.size().x;
                let x = br.center().x - total / 2.0;
                let ir = Rect::from_min_size(Pos2::new(x, br.center().y - icon_s / 2.0), Vec2::splat(icon_s));
                self.draw_icon(ui, &item_id, &IconSrc::Builtin(m.icon), ir, color);
                let pos = Pos2::new(ir.right() + 7.0 * self.scale, br.center().y - g.size().y / 2.0);
                self.text(ui, pos, g, color, on_image && !selected);
            } else {
                let ir = Rect::from_center_size(br.center(), Vec2::splat(icon_s));
                self.draw_icon(ui, &item_id, &IconSrc::Builtin(m.icon), ir, color);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_list(&mut self, ui: &mut Ui, rect: Rect, cols: usize, lines: usize, gap: f32, el: &ElStyle, out: &mut Output) {
        let items: &'a [Item] = self.items;
        let cols = cols.max(1);
        let lines = lines.max(1);
        let gap = gap * self.scale;
        let rh = self.row_height(el);
        let cw = (rect.width() - gap * (cols - 1) as f32) / cols as f32;

        // Keep the selected row on screen.
        let sel_row = self.selected / cols;
        let mut first = self.first_row;
        if sel_row < first {
            first = sel_row;
        }
        if sel_row >= first + lines {
            first = sel_row + 1 - lines;
        }
        let total_rows = items.len().div_ceil(cols);
        if total_rows <= lines {
            first = 0;
        } else {
            first = first.min(total_rows - lines);
        }
        out.first_row = first;

        if self.interactive {
            let resp = ui.interact(rect, Id::new(("hestia-list", self.id_salt)), Sense::hover());
            if resp.hovered() {
                let dy = ui.input(|i| i.raw_scroll_delta.y);
                if dy > 0.5 {
                    out.scroll = -1;
                } else if dy < -0.5 {
                    out.scroll = 1;
                }
            }
        }

        if items.is_empty() {
            let color = self.role(Role::Fg).gamma_multiply(0.6);
            let g = text_galley(ui, "Nothing found", self.font(1.0), color, rect.width());
            ui.painter().galley(Pos2::new(rect.center().x - g.size().x / 2.0, rect.top() + 20.0 * self.scale), g, color);
            return;
        }

        let pad = self.mg(el.pad);
        let icon = el.icon * self.scale;
        let rd = self.rd(el.round);
        // Round icon tiles: a circle around the icon with the label underneath.
        let circle_tiles = el.vertical && self.opts.button_shape == ButtonShape::Circle;
        // "Bubbles" in lists: a round bubble behind the icon instead of a highlighted row.
        let bubbles = !el.vertical && self.opts.button_shape == ButtonShape::Circle;
        let _ = cw;
        for row in 0..lines {
            // In shaped windows each row is trimmed to fit inside the outline.
            let row_rect = self.fit(Rect::from_min_size(
                Pos2::new(rect.left(), rect.top() + row as f32 * (rh + gap)),
                Vec2::new(rect.width(), rh),
            ));
            let cw = ((row_rect.width() - gap * (cols - 1) as f32) / cols as f32).max(0.0);
            if cw < 20.0 * self.scale {
                continue;
            }
            for col in 0..cols {
                let idx = (first + row) * cols + col;
                let cell =
                    Rect::from_min_size(Pos2::new(row_rect.left() + col as f32 * (cw + gap), row_rect.top()), Vec2::new(cw, rh));
                let th = if el.label { el.gap * self.scale + self.text_h(el.font) } else { 0.0 };
                let circle = {
                    let r = (icon / 2.0 + 16.0 * self.scale).min((cell.height() - th) / 2.0).min(cell.width() / 2.0);
                    let cy = cell.center().y - th / 2.0;
                    (Pos2::new(cell.center().x, cy), r)
                };
                let bubble = (Pos2::new(cell.left() + pad.left + icon / 2.0, cell.center().y), icon / 2.0 + 7.0 * self.scale);
                let paint_bg = |s: &Self, color: Color32| {
                    if bubbles {
                        ui.painter().circle_filled(bubble.0, bubble.1, color);
                    } else if circle_tiles {
                        ui.painter().circle_filled(circle.0, circle.1, color);
                    } else {
                        s.shape_fill(ui, cell, rd, color);
                    }
                };
                let Some(item) = items.get(idx) else {
                    // Empty slots still show their tile background in tile layouts.
                    if let (Some(f), false) = (el.fill, bubbles) {
                        paint_bg(self, self.c(Fill(f.0, f.1 * 0.4)));
                    }
                    continue;
                };
                let selected = idx == self.selected;
                let mut hovered = false;
                if self.interactive {
                    let resp = ui.interact(cell, Id::new(("hestia-item", self.id_salt, idx)), Sense::click());
                    hovered = resp.hovered();
                    if resp.clicked() {
                        out.clicked = Some(idx);
                    }
                    if resp.secondary_clicked() {
                        out.context = Some((idx, resp.interact_pointer_pos().unwrap_or(cell.center())));
                    }
                }
                let bg = if selected {
                    el.sel_fill
                } else if hovered {
                    el.sel_fill.map(|f| Fill(f.0, 0.3)).or(el.fill)
                } else {
                    el.fill
                };
                if let Some(f) = bg {
                    paint_bg(self, self.c(f));
                }
                let mut color = self.role(if selected { el.sel_text } else { el.text });
                if circle_tiles && !selected {
                    color = self.role(el.text);
                }
                // With bubbles only the icon sits on the highlight; the label stays on the panel.
                let label_color = if bubbles { self.role(el.text) } else { color };
                let mut inner = cell - pad;
                if !el.vertical {
                    let inset = self.shape_inset(cell.height());
                    inner.min.x += inset;
                    inner.max.x -= inset;
                }
                if el.vertical {
                    let top = inner.center().y - (icon + th) / 2.0;
                    let ir = if circle_tiles {
                        Rect::from_center_size(circle.0, Vec2::splat(icon))
                    } else {
                        Rect::from_min_size(Pos2::new(inner.center().x - icon / 2.0, top), Vec2::splat(icon))
                    };
                    self.draw_icon(ui, &item.id, &item.icon, ir, color);
                    if el.label {
                        // Under a round tile the label sits on the panel, so use normal text colour.
                        let lc = if circle_tiles { self.role(el.text) } else { color };
                        let g = text_galley(ui, &item.label, self.font(el.font), lc, inner.width());
                        let y = if circle_tiles {
                            circle.0.y + circle.1 + 6.0 * self.scale
                        } else {
                            ir.bottom() + el.gap * self.scale
                        };
                        ui.painter().galley(Pos2::new(inner.center().x - g.size().x / 2.0, y), g, lc);
                    }
                } else {
                    let ir = Rect::from_min_size(Pos2::new(inner.left(), inner.center().y - icon / 2.0), Vec2::splat(icon));
                    self.draw_icon(ui, &item.id, &item.icon, ir, color);
                    if el.label {
                        let x = ir.right() + el.gap * self.scale + if bubbles { 4.0 * self.scale } else { 0.0 };
                        let avail = (inner.right() - x).max(0.0);
                        let font = if bubbles && selected { self.bold(el.font) } else { self.font(el.font) };
                        let color = label_color;
                        let g = text_galley(ui, &item.label, font, color, avail);
                        let gw = g.size().x;
                        ui.painter().galley(Pos2::new(x, inner.center().y - g.size().y / 2.0), g, color);
                        if !item.sub.is_empty() && avail - gw > 40.0 * self.scale {
                            let sc = color.gamma_multiply(0.6);
                            let sg = text_galley(ui, &format!("  ·  {}", item.sub), self.font(el.font * 0.85), sc, avail - gw);
                            ui.painter().galley(Pos2::new(x + gw, inner.center().y - sg.size().y / 2.0), sg, sc);
                        }
                    }
                }
            }
        }
    }
}

/// Lay out one line of text, cutting it off with "…" if it doesn't fit.
pub fn text_galley(ui: &Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping::truncate_at_width(max_w.max(1.0));
    ui.painter().layout_job(job)
}

/// Paint a convex polygon filled with part of a texture (`uv` is what `rect` shows).
fn paint_textured(ui: &Ui, tex: egui::TextureId, poly: &[Pos2], rect: Rect, uv: Rect, tint: Color32) {
    let map = |p: Pos2| {
        Pos2::new(
            uv.min.x + (p.x - rect.min.x) / rect.width() * uv.width(),
            uv.min.y + (p.y - rect.min.y) / rect.height() * uv.height(),
        )
    };
    let n = poly.len();
    if n < 3 {
        return;
    }
    let centre = Pos2::new(poly.iter().map(|p| p.x).sum::<f32>() / n as f32, poly.iter().map(|p| p.y).sum::<f32>() / n as f32);
    let mut mesh = egui::Mesh::with_texture(tex);
    mesh.vertices.push(egui::epaint::Vertex { pos: centre, uv: map(centre), color: tint });
    for p in poly {
        mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv: map(*p), color: tint });
    }
    let n = n as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// A circle (sides < 3) or regular polygon with a flat bottom edge, centred on `c`.
pub fn poly_path(sides: u8, c: Pos2, rad: f32) -> Vec<Pos2> {
    use std::f32::consts::PI;
    if sides < 3 {
        return (0..96)
            .map(|i| {
                let a = i as f32 / 96.0 * 2.0 * PI;
                Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())
            })
            .collect();
    }
    let n = sides as f32;
    (0..sides)
        .map(|k| {
            let a = PI / 2.0 + PI / n + 2.0 * PI * k as f32 / n;
            Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())
        })
        .collect()
}

/// cos(π/n): how far the flat sides of a polygon are from its centre (1 for a circle).
pub fn inradius_factor(sides: u8) -> f32 {
    if sides < 3 {
        1.0
    } else {
        (std::f32::consts::PI / sides as f32).cos()
    }
}
