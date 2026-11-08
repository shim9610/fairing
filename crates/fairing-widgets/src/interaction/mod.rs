//! Retained interaction state has an owner and a render-pass lifetime.
pub(crate) mod concrete;
pub(crate) mod rules;
pub(crate) use concrete::Lifetime;
use concrete::{Pass, Session};
use rules::Snapshot;

pub(crate) fn load<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    id: egui::Id,
    enabled: bool,
    lifetime: Lifetime,
) -> Option<T> {
    load_by(ctx, id, enabled, lifetime, concrete::restore::<T>)
}

pub(crate) fn load_capture<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    id: egui::Id,
    enabled: bool,
    lifetime: Lifetime,
) -> Option<concrete::Capture<T>> {
    load_by(ctx, id, enabled, lifetime, concrete::restore_capture::<T>)
}

fn load_by<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    id: egui::Id,
    enabled: bool,
    lifetime: Lifetime,
    select: for<'a> fn(Option<&'a Session<T>>, Pass) -> Option<&'a T>,
) -> Option<T> {
    let now = ctx.cumulative_pass_nr();
    let fresh_press = ctx.input(|i| i.pointer.primary_pressed());
    ctx.data_mut(|data| {
        let raw = egui::util::id_type_map::RawKey::new::<Session<T>>(id);
        let payload = data
            .get_temp_raw(raw)
            .and_then(|value| value.downcast_ref::<Session<T>>())
            .and_then(|saved| {
                select(
                    Some(saved),
                    Pass {
                        now,
                        enabled,
                        lifetime,
                        fresh_press,
                    },
                )
            })
            .cloned();
        if payload.is_none() {
            data.remove::<Session<T>>(id);
        }
        payload
    })
}

pub(crate) fn store<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    id: egui::Id,
    payload: T,
) {
    let seen = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| data.insert_temp(id, Session { seen, payload }));
}

pub(crate) fn take<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    id: egui::Id,
) -> Option<T> {
    let raw = egui::util::id_type_map::RawKey::new::<Session<T>>(id);
    ctx.data_mut(|data| data.remove_temp_raw(raw))
        .and_then(|saved| saved.downcast::<Session<T>>().ok())
        .map(|saved| saved.payload)
}

pub(crate) fn forget_focus(ctx: &egui::Context, id: egui::Id) {
    ctx.memory_mut(|memory| memory.surrender_focus(id));
    if ctx.dragged_id() == Some(id) {
        ctx.stop_dragging();
    }
}

pub(crate) fn retire_area(ctx: &egui::Context, layer: egui::LayerId) {
    if let Some(mut state) = egui::AreaState::load(ctx, layer.id) {
        state.interactable = false;
        ctx.memory_mut(|memory| memory.areas_mut().set_state(layer, state));
    }
}

pub(crate) fn snapshot(active: bool) -> Snapshot {
    Snapshot {
        present: true,
        enabled: true,
        continuous: true,
        active,
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn pointer(response: &egui::Response) -> concrete::Pointer {
    let (fresh, down) = response.ctx.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
        )
    });
    concrete::Pointer {
        fresh,
        down,
        held: response.is_pointer_button_down_on(),
        clicked: response.clicked(),
        dragged: response.dragged(),
        stopped: response.drag_stopped(),
    }
}
