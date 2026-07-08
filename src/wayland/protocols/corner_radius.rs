use cosmic_protocols::corner_radius::v1::server::cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1;
use cosmic_protocols::corner_radius::v1::server::cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1;
use cosmic_protocols::corner_radius::v1::server::{
    cosmic_corner_radius_layer_v1, cosmic_corner_radius_manager_v1,
    cosmic_corner_radius_toplevel_v1,
};
use smithay::utils::HookId;
use smithay::wayland::compositor::Cacheable;
use smithay::wayland::compositor::add_pre_commit_hook;
use smithay::wayland::compositor::remove_pre_commit_hook;
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::SurfaceCachedState;
use smithay::{
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_surface::XdgSurface,
        wayland_protocols_wlr::layer_shell::v1::server::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, Weak,
            protocol::wl_surface::WlSurface,
        },
    },
    wayland::shell::xdg::XdgShellHandler,
};
use std::sync::Mutex;
use wayland_backend::server::GlobalId;

type ToplevelHookId = Mutex<Option<(HookId, Weak<CosmicCornerRadiusToplevelV1>)>>;
type LayerObject = Mutex<Option<Weak<CosmicCornerRadiusLayerV1>>>;

#[derive(Debug)]
pub struct CornerRadiusState {
    instances: Vec<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1>,
    global: GlobalId,
}

impl CornerRadiusState {
    pub fn new<D>(dh: &DisplayHandle) -> CornerRadiusState
    where
        D: GlobalDispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
            + Dispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
            + Dispatch<
                cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1,
                CornerRadiusData,
            > + Dispatch<cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1, CornerRadiusData>
            + CornerRadiusHandler
            + 'static,
    {
        let global = dh
            .create_global::<D, cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, _>(
                2,
                (),
            );
        CornerRadiusState {
            instances: Vec::new(),
            global,
        }
    }

    pub fn global_id(&self) -> GlobalId {
        self.global.clone()
    }
}

pub trait CornerRadiusHandler: XdgShellHandler {
    fn corner_radius_state(&mut self) -> &mut CornerRadiusState;
    fn xdg_wl_surface(&self, surface: &XdgSurface) -> Option<WlSurface>;
    fn layer_wl_surface(&self, surface: &ZwlrLayerSurfaceV1) -> Option<WlSurface>;
    fn set_corner_radius(&mut self, data: &CornerRadiusData);
    fn unset_corner_radius(&mut self, data: &CornerRadiusData);
}

impl<D> GlobalDispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, (), D>
    for CornerRadiusState
where
    D: GlobalDispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
        + Dispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
        + Dispatch<cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1, CornerRadiusData>
        + Dispatch<cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1, CornerRadiusData>
        + CornerRadiusHandler
        + 'static,
{
    fn bind(
        state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: smithay::reexports::wayland_server::New<
            cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1,
        >,
        _global_data: &(),
        data_init: &mut smithay::reexports::wayland_server::DataInit<'_, D>,
    ) {
        let instance = data_init.init(resource, ());
        state.corner_radius_state().instances.push(instance);
    }
}

impl<D> Dispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, (), D>
    for CornerRadiusState
where
    D: GlobalDispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
        + Dispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
        + Dispatch<cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1, CornerRadiusData>
        + Dispatch<cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1, CornerRadiusData>
        + CornerRadiusHandler
        + 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        resource: &cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1,
        request: <cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1 as smithay::reexports::wayland_server::Resource>::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        data_init: &mut smithay::reexports::wayland_server::DataInit<'_, D>,
    ) {
        match request {
            cosmic_corner_radius_manager_v1::Request::Destroy => {
                let corner_radius_state = state.corner_radius_state();
                corner_radius_state.instances.retain(|i| i != resource);
            }
            cosmic_corner_radius_manager_v1::Request::GetCornerRadius { id, toplevel } => {
                if let Some(surface) = state.xdg_shell_state().get_toplevel(&toplevel) {
                    create_xdg_corner_radius::<D>(resource, id, surface.wl_surface(), data_init);
                } else {
                    // The toplevel is not known (e.g. already destroyed); every `New` resource
                    // must still be initialized, so create an inert object.
                    init_inert(id, data_init);
                }
            }
            cosmic_corner_radius_manager_v1::Request::GetCornerRadiusSurface { id, surface } => {
                let Some(surface) = state.xdg_wl_surface(&surface) else {
                    resource.post_error(
                        cosmic_corner_radius_manager_v1::Error::NoRole as u32,
                        "xdg_surface has no active toplevel or popup role",
                    );
                    init_inert(id, data_init);
                    return;
                };
                create_xdg_corner_radius::<D>(resource, id, &surface, data_init);
            }
            cosmic_corner_radius_manager_v1::Request::GetCornerRadiusLayer { id, layer } => {
                if let Some(surface) = state.layer_wl_surface(&layer) {
                    create_layer_corner_radius(resource, id, &surface, data_init);
                } else {
                    init_inert(id, data_init);
                }
            }
            _ => unreachable!(),
        }
    }

    fn destroyed(
        state: &mut D,
        _client: wayland_backend::server::ClientId,
        resource: &cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1,
        _data: &(),
    ) {
        let corner_radius_state = state.corner_radius_state();
        corner_radius_state.instances.retain(|i| i != resource);
    }
}

/// Initializes an inert corner-radius object.
///
/// Every `New` resource has to be initialized, even on protocol-error paths,
/// as failing to do so panics inside wayland-server.
fn init_inert<D, I>(id: New<I>, data_init: &mut DataInit<'_, D>)
where
    D: Dispatch<I, CornerRadiusData> + 'static,
    I: Resource + 'static,
{
    data_init.init(
        id,
        Mutex::new(CornerRadiusInternal {
            surface: None,
            corners: None,
            padding: None,
        }),
    );
}

fn create_xdg_corner_radius<D>(
    manager: &cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1,
    id: New<CosmicCornerRadiusToplevelV1>,
    surface: &WlSurface,
    data_init: &mut DataInit<'_, D>,
) where
    D: Dispatch<CosmicCornerRadiusToplevelV1, CornerRadiusData> + CornerRadiusHandler + 'static,
{
    // Take out any previous hook. If its object is still alive, creating a
    // second corner-radius object for the surface is a protocol error.
    let previous_hook = with_states(surface, |surface_data| {
        let hook = surface_data
            .data_map
            .get_or_insert_threadsafe(|| ToplevelHookId::new(None));
        let mut guard = hook.lock().unwrap();
        if guard
            .as_ref()
            .is_some_and(|(_, object)| object.upgrade().is_ok())
        {
            Err(())
        } else {
            Ok(guard.take())
        }
    });
    let previous_hook = match previous_hook {
        Ok(previous_hook) => previous_hook,
        Err(()) => {
            manager.post_error(
                cosmic_corner_radius_manager_v1::Error::CornerRadiusExists as u32,
                format!("{manager:?} corner-radius object already exists for the surface"),
            );
            init_inert(id, data_init);
            return;
        }
    };
    // The previous object is dead; unregister its stale hook before adding a new one.
    if let Some((hook_id, _)) = previous_hook {
        remove_pre_commit_hook(surface, &hook_id);
    }

    let data = Mutex::new(CornerRadiusInternal {
        surface: Some(surface.downgrade()),
        corners: None,
        padding: None,
    });
    let object = data_init.init(id, data);
    let weak_object = object.downgrade();
    let hook_id = add_pre_commit_hook::<D, _>(surface, move |_, _dh, surface| {
        let corner_radii_too_big = with_states(surface, |surface_data| {
            let corners = *surface_data
                .cached_state
                .get::<CacheableCorners>()
                .pending();
            surface_data
                .cached_state
                .get::<SurfaceCachedState>()
                .pending()
                .geometry
                .zip(corners.0.as_ref())
                .is_some_and(|(geometry, corners)| {
                    let half_min_dim =
                        u8::try_from(geometry.size.w.min(geometry.size.h) / 2).unwrap_or(u8::MAX);
                    corners.top_right > half_min_dim
                        || corners.top_left > half_min_dim
                        || corners.bottom_right > half_min_dim
                        || corners.bottom_left > half_min_dim
                })
        });

        if corner_radii_too_big {
            object.post_error(
                cosmic_corner_radius_toplevel_v1::Error::RadiusTooLarge as u32,
                format!("{object:?} corner radius too large"),
            );
        }
    });

    with_states(surface, |surface_data| {
        let hook = surface_data
            .data_map
            .get_or_insert_threadsafe(|| ToplevelHookId::new(None));
        *hook.lock().unwrap() = Some((hook_id, weak_object));
    });
}

fn create_layer_corner_radius<D>(
    manager: &cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1,
    id: New<CosmicCornerRadiusLayerV1>,
    surface: &WlSurface,
    data_init: &mut DataInit<'_, D>,
) where
    D: Dispatch<CosmicCornerRadiusLayerV1, CornerRadiusData> + 'static,
{
    let radius_exists = with_states(surface, |surface_data| {
        let object = surface_data
            .data_map
            .get_or_insert_threadsafe(|| LayerObject::new(None));
        object
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|object| object.upgrade().is_ok())
    });
    if radius_exists {
        manager.post_error(
            cosmic_corner_radius_manager_v1::Error::CornerRadiusExists as u32,
            format!("{manager:?} corner-radius object already exists for the layer surface"),
        );
        init_inert(id, data_init);
        return;
    }

    let data = Mutex::new(CornerRadiusInternal {
        surface: Some(surface.downgrade()),
        corners: None,
        padding: None,
    });
    let object = data_init.init(id, data);
    with_states(surface, |surface_data| {
        let current = surface_data
            .data_map
            .get_or_insert_threadsafe(|| LayerObject::new(None));
        *current.lock().unwrap() = Some(object.downgrade());
    });
}

impl<D>
    Dispatch<cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1, CornerRadiusData, D>
    for CornerRadiusState
where
    D: GlobalDispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
        + Dispatch<cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1, ()>
        + Dispatch<cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1, CornerRadiusData>
        + CornerRadiusHandler
        + 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        resource: &cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1,
        request: <cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1 as Resource>::Request,
        data: &CornerRadiusData,
        _dhandle: &DisplayHandle,
        _data_init: &mut smithay::reexports::wayland_server::DataInit<'_, D>,
    ) {
        match request {
            cosmic_corner_radius_toplevel_v1::Request::Destroy => {
                let mut guard = data.lock().unwrap();
                guard.corners = None;

                let Some(surface) = guard.wl_surface() else {
                    return;
                };

                let hook = with_states(&surface, |surface_data| {
                    *surface_data
                        .cached_state
                        .get::<CacheableCorners>()
                        .pending() = CacheableCorners(None);
                    surface_data
                        .data_map
                        .get::<ToplevelHookId>()
                        .and_then(|hook| hook.lock().unwrap().take())
                });
                if let Some((hook_id, _)) = hook {
                    remove_pre_commit_hook(&surface, &hook_id);
                }
                drop(guard);

                state.unset_corner_radius(data);
            }
            cosmic_corner_radius_toplevel_v1::Request::SetRadius {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            } => {
                let mut guard = data.lock().unwrap();
                guard.set_corner_radius(top_left, top_right, bottom_right, bottom_left);
                let Some(surface) = guard.wl_surface() else {
                    resource.post_error(
                        cosmic_corner_radius_toplevel_v1::Error::ToplevelDestroyed as u32,
                        format!("{resource:?} associated xdg_surface was destroyed"),
                    );
                    return;
                };

                with_states(&surface, |surface_data| {
                    *surface_data
                        .cached_state
                        .get::<CacheableCorners>()
                        .pending() = CacheableCorners(guard.corners);
                });
                drop(guard);

                state.set_corner_radius(data);
            }
            cosmic_corner_radius_toplevel_v1::Request::UnsetRadius => {
                let mut guard = data.lock().unwrap();
                guard.corners = None;
                let Some(surface) = guard.wl_surface() else {
                    resource.post_error(
                        cosmic_corner_radius_toplevel_v1::Error::ToplevelDestroyed as u32,
                        format!("{resource:?} associated xdg_surface was destroyed"),
                    );
                    return;
                };

                with_states(&surface, |surface_data| {
                    *surface_data
                        .cached_state
                        .get::<CacheableCorners>()
                        .pending() = CacheableCorners(None);
                });
                drop(guard);

                state.unset_corner_radius(data);
            }
            _ => unimplemented!(),
        }
    }
}

impl<D> Dispatch<cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1, CornerRadiusData, D>
    for CornerRadiusState
where
    D: Dispatch<cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1, CornerRadiusData>
        + CornerRadiusHandler
        + 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        resource: &cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1,
        request: <cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1 as Resource>::Request,
        data: &CornerRadiusData,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            cosmic_corner_radius_layer_v1::Request::Destroy => {
                let mut guard = data.lock().unwrap();
                guard.corners = None;
                guard.padding = None;
                let Some(surface) = guard.wl_surface() else {
                    return;
                };
                with_states(&surface, |surface_data| {
                    if let Some(object) = surface_data.data_map.get::<LayerObject>() {
                        *object.lock().unwrap() = None;
                    }
                    *surface_data
                        .cached_state
                        .get::<CacheableCorners>()
                        .pending() = CacheableCorners(None);
                    *surface_data
                        .cached_state
                        .get::<CacheablePadding>()
                        .pending() = CacheablePadding(None);
                });
                drop(guard);
                state.unset_corner_radius(data);
            }
            cosmic_corner_radius_layer_v1::Request::SetRadius {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            } => {
                let mut guard = data.lock().unwrap();
                guard.set_corner_radius(top_left, top_right, bottom_right, bottom_left);
                let Some(surface) = guard.wl_surface() else {
                    resource.post_error(
                        cosmic_corner_radius_layer_v1::Error::LayerDestroyed as u32,
                        format!("{resource:?} associated layer surface was destroyed"),
                    );
                    return;
                };
                with_states(&surface, |surface_data| {
                    *surface_data
                        .cached_state
                        .get::<CacheableCorners>()
                        .pending() = CacheableCorners(guard.corners);
                });
                drop(guard);
                state.set_corner_radius(data);
            }
            cosmic_corner_radius_layer_v1::Request::UnsetRadius => {
                let mut guard = data.lock().unwrap();
                guard.corners = None;
                let Some(surface) = guard.wl_surface() else {
                    resource.post_error(
                        cosmic_corner_radius_layer_v1::Error::LayerDestroyed as u32,
                        format!("{resource:?} associated layer surface was destroyed"),
                    );
                    return;
                };
                with_states(&surface, |surface_data| {
                    *surface_data
                        .cached_state
                        .get::<CacheableCorners>()
                        .pending() = CacheableCorners(None);
                });
                drop(guard);
                state.unset_corner_radius(data);
            }
            cosmic_corner_radius_layer_v1::Request::SetPadding {
                top,
                right,
                bottom,
                left,
            } => {
                let mut guard = data.lock().unwrap();
                guard.padding = Some(Padding {
                    top,
                    right,
                    bottom,
                    left,
                });
                let Some(surface) = guard.wl_surface() else {
                    resource.post_error(
                        cosmic_corner_radius_layer_v1::Error::LayerDestroyed as u32,
                        format!("{resource:?} associated layer surface was destroyed"),
                    );
                    return;
                };
                with_states(&surface, |surface_data| {
                    *surface_data
                        .cached_state
                        .get::<CacheablePadding>()
                        .pending() = CacheablePadding(guard.padding);
                });
                drop(guard);
                state.set_corner_radius(data);
            }
            cosmic_corner_radius_layer_v1::Request::UnsetPadding => {
                let mut guard = data.lock().unwrap();
                guard.padding = None;
                let Some(surface) = guard.wl_surface() else {
                    resource.post_error(
                        cosmic_corner_radius_layer_v1::Error::LayerDestroyed as u32,
                        format!("{resource:?} associated layer surface was destroyed"),
                    );
                    return;
                };
                with_states(&surface, |surface_data| {
                    *surface_data
                        .cached_state
                        .get::<CacheablePadding>()
                        .pending() = CacheablePadding(None);
                });
                drop(guard);
                state.unset_corner_radius(data);
            }
            _ => unreachable!(),
        }
    }
}

pub type CornerRadiusData = Mutex<CornerRadiusInternal>;

#[derive(Debug)]
pub struct CornerRadiusInternal {
    /// The associated surface. `None` for inert objects created on error paths.
    pub surface: Option<Weak<WlSurface>>,
    pub corners: Option<Corners>,
    pub padding: Option<Padding>,
}

#[derive(Debug, Copy, Clone)]
pub struct Corners {
    pub top_left: u8,
    pub top_right: u8,
    pub bottom_right: u8,
    pub bottom_left: u8,
}

#[derive(Debug, Copy, Clone)]
pub struct Padding {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

#[derive(Default, Debug, Copy, Clone)]
pub struct CacheableCorners(pub Option<Corners>);

#[derive(Default, Debug, Copy, Clone)]
pub struct CacheablePadding(pub Option<Padding>);

impl Cacheable for CacheableCorners {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        *self
    }
    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

impl Cacheable for CacheablePadding {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        *self
    }
    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

impl CornerRadiusInternal {
    pub fn wl_surface(&self) -> Option<WlSurface> {
        self.surface.as_ref().and_then(|s| s.upgrade().ok())
    }

    fn set_corner_radius(
        &mut self,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) {
        let corners = Corners {
            top_left: top_left.clamp(u8::MIN as u32, u8::MAX as u32) as u8,
            top_right: top_right.clamp(u8::MIN as u32, u8::MAX as u32) as u8,
            bottom_right: bottom_right.clamp(u8::MIN as u32, u8::MAX as u32) as u8,
            bottom_left: bottom_left.clamp(u8::MIN as u32, u8::MAX as u32) as u8,
        };
        self.corners = Some(corners);
    }
}

macro_rules! delegate_corner_radius {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        smithay::reexports::wayland_server::delegate_global_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            cosmic_protocols::corner_radius::v1::server::cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1: ()
        ] => $crate::wayland::protocols::corner_radius::CornerRadiusState);
        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            cosmic_protocols::corner_radius::v1::server::cosmic_corner_radius_manager_v1::CosmicCornerRadiusManagerV1: ()
        ] => $crate::wayland::protocols::corner_radius::CornerRadiusState);
        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            cosmic_protocols::corner_radius::v1::server::cosmic_corner_radius_toplevel_v1::CosmicCornerRadiusToplevelV1: CornerRadiusData
        ] => $crate::wayland::protocols::corner_radius::CornerRadiusState);
        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            cosmic_protocols::corner_radius::v1::server::cosmic_corner_radius_layer_v1::CosmicCornerRadiusLayerV1: CornerRadiusData
        ] => $crate::wayland::protocols::corner_radius::CornerRadiusState);
    };
}
pub(crate) use delegate_corner_radius;
