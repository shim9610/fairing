//! Retained interaction state has an owner and a render-pass lifetime.
pub(crate) mod rules;
use rules::{Operation, Snapshot};

#[derive(Clone, Copy)]
pub(crate) enum Lifetime {
    Mounted,
    Press,
}

#[derive(Clone)]
struct Session<T> {
    seen: u64,
    payload: T,
}

pub(crate) fn load<T: Clone + Send + Sync + 'static>(
    ctx: &egui::Context,
    id: egui::Id,
    enabled: bool,
    lifetime: Lifetime,
) -> Option<T> {
    let now = ctx.cumulative_pass_nr();
    let fresh_press = ctx.input(|i| i.pointer.primary_pressed());
    ctx.data_mut(|data| {
        let raw = egui::util::id_type_map::RawKey::new::<Session<T>>(id);
        let keep = data
            .get_temp_raw(raw)
            .and_then(|value| value.downcast_ref::<Session<T>>())
            .is_some_and(|saved| {
                let continuous = rules::live(
                    saved.seen,
                    now,
                    matches!(lifetime, Lifetime::Press),
                    fresh_press,
                );
                rules::step(
                    Snapshot {
                        present: true,
                        enabled,
                        continuous,
                        active: true,
                    },
                    Operation::Observe,
                )
                .active
            });
        if keep {
            data.get_temp_raw(raw)
                .and_then(|value| value.downcast_ref::<Session<T>>())
                .map(|saved| saved.payload.clone())
        } else {
            data.remove::<Session<T>>(id);
            None
        }
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
