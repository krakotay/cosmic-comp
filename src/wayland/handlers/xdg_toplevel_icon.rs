// SPDX-License-Identifier: GPL-3.0-only

use smithay::wayland::xdg_toplevel_icon::XdgToplevelIconHandler;

use crate::state::State;

// The icon is stored in the surface's double-buffered
// `ToplevelIconCachedState`; nothing else to do until we consume it.
impl XdgToplevelIconHandler for State {}
