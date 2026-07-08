use smithay::{
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_surface::XdgSurface,
        wayland_protocols_wlr::layer_shell::v1::server::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        wayland_server::{Resource, protocol::wl_surface::WlSurface},
    },
    wayland::shell::xdg::XdgShellSurfaceUserData,
};

use crate::wayland::protocols::corner_radius::{
    CornerRadiusData, CornerRadiusHandler, CornerRadiusState, delegate_corner_radius,
};

use crate::state::State;

impl CornerRadiusHandler for State {
    fn corner_radius_state(&mut self) -> &mut CornerRadiusState {
        &mut self.common.corner_radius_state
    }

    fn xdg_wl_surface(&self, resource: &XdgSurface) -> Option<WlSurface> {
        self.common
            .xdg_shell_state
            .toplevel_surfaces()
            .iter()
            .find_map(|surface| {
                let data = surface.xdg_toplevel().data::<XdgShellSurfaceUserData>()?;
                (data.xdg_surface() == resource).then(|| surface.wl_surface().clone())
            })
            .or_else(|| {
                self.common
                    .xdg_shell_state
                    .popup_surfaces()
                    .iter()
                    .find_map(|surface| {
                        let data = surface.xdg_popup().data::<XdgShellSurfaceUserData>()?;
                        (data.xdg_surface() == resource).then(|| surface.wl_surface().clone())
                    })
            })
    }

    fn layer_wl_surface(&self, resource: &ZwlrLayerSurfaceV1) -> Option<WlSurface> {
        self.common
            .layer_shell_state
            .layer_surfaces()
            .find(|surface| surface.shell_surface() == resource)
            .map(|surface| surface.wl_surface().clone())
    }

    fn set_corner_radius(&mut self, data: &CornerRadiusData) {
        if force_redraw(self, data).is_none() {
            tracing::warn!("Failed to force redraw for corner radius change.");
        }
    }

    fn unset_corner_radius(&mut self, data: &CornerRadiusData) {
        if force_redraw(self, data).is_none() {
            tracing::warn!("Failed to force redraw for corner radius reset.");
        }
    }
}

fn force_redraw(state: &mut State, data: &CornerRadiusData) -> Option<()> {
    let guard = data.lock().unwrap();

    let surface = guard.surface.upgrade().ok()?;

    let guard = state.common.shell.read();
    let output = guard.visible_output_for_surface(&surface)?;

    state.backend.schedule_render(output);
    Some(())
}

delegate_corner_radius!(State);
