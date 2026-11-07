//! Conservative compiled-MIR contracts for permission declaration/mapping methods.
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_hir::definitions::DefPathData;
use rustc_middle::{
    mir,
    ty::{self, TyCtxt},
};
use std::collections::HashSet;

fn qualified(tcx: TyCtxt<'_>, id: DefId) -> String {
    if id.is_local() {
        format!("{}::{}", tcx.crate_name(id.krate), tcx.def_path_str(id))
    } else {
        tcx.def_path_str(id)
    }
}
// Diagnostic paths may use a public re-export in a downstream crate. Security
// subjects are matched using the compiler's definition path, not pretty printing.
pub(super) fn definition_path(tcx: TyCtxt<'_>, id: DefId) -> String {
    let parts = tcx
        .def_path(id)
        .data
        .into_iter()
        .map(|part| match part.data {
            DefPathData::TypeNs(name) | DefPathData::ValueNs(name) => name.to_string(),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>();
    format!("{}::{}", tcx.crate_name(id.krate), parts.join("::"))
}

fn check_body(
    tcx: TyCtxt<'_>,
    def: DefId,
    active: &mut HashSet<DefId>,
    done: &mut HashSet<DefId>,
) -> Result<(), String> {
    if done.contains(&def) {
        return Ok(());
    }
    if active.len() >= 128 || !active.insert(def) {
        return Err(format!(
            "recursive or excessively deep metadata helper: {}",
            qualified(tcx, def)
        ));
    }
    if tcx.trait_of_assoc(def).is_some() {
        return Err(format!(
            "unresolved trait dispatch: {}",
            qualified(tcx, def)
        ));
    }
    if matches!(tcx.def_kind(def), DefKind::Ctor(..)) {
        active.remove(&def);
        done.insert(def);
        return Ok(());
    }
    let body = if tcx.is_mir_available(def) {
        tcx.optimized_mir(def)
    } else if tcx.is_const_fn(def) {
        // Public const constructors retain their CTFE MIR in dependency metadata
        // even when optimized runtime MIR is not exported. Analyze that body too.
        tcx.mir_for_ctfe(def)
    } else {
        return Err(format!("callee MIR unavailable: {}", qualified(tcx, def)));
    };
    if body
        .local_decls
        .iter()
        .any(|local| matches!(local.ty.kind(), ty::RawPtr(..)))
    {
        return Err(format!("raw-pointer operation in {}", qualified(tcx, def)));
    }
    let writes_external = |place: &mir::Place<'_>| {
        place
            .projection
            .iter()
            .any(|p| matches!(p, mir::ProjectionElem::Deref))
    };
    for block in body.basic_blocks.iter() {
        for statement in &block.statements {
            match &statement.kind {
                mir::StatementKind::Assign(assignment) => {
                    if writes_external(&assignment.0) {
                        return Err(format!(
                            "write through a reference in {}",
                            qualified(tcx, def)
                        ));
                    }
                    match &assignment.1 {
                        mir::Rvalue::Use(..)
                        | mir::Rvalue::Ref(..)
                        | mir::Rvalue::Aggregate(..)
                        | mir::Rvalue::Discriminant(..)
                        | mir::Rvalue::Cast(..)
                        | mir::Rvalue::BinaryOp(..)
                        | mir::Rvalue::UnaryOp(..)
                        | mir::Rvalue::CopyForDeref(..) => {}
                        other => {
                            return Err(format!(
                                "unsupported metadata expression {other:?} in {}",
                                qualified(tcx, def)
                            ))
                        }
                    }
                }
                mir::StatementKind::StorageLive(..)
                | mir::StatementKind::StorageDead(..)
                | mir::StatementKind::Nop
                | mir::StatementKind::PlaceMention(..)
                | mir::StatementKind::ConstEvalCounter => {}
                other => {
                    return Err(format!(
                        "unsupported metadata statement {other:?} in {}",
                        qualified(tcx, def)
                    ))
                }
            }
        }
        match &block.terminator().kind {
            mir::TerminatorKind::Call {
                func, destination, ..
            } => {
                if writes_external(&destination) {
                    return Err("call result writes through a reference".into());
                }
                let ty = func.ty(&body.local_decls, tcx);
                let ty::FnDef(callee, _) = ty.kind() else {
                    return Err("indirect metadata function call".into());
                };
                check_body(tcx, *callee, active, done)?;
            }
            mir::TerminatorKind::Goto { .. }
            | mir::TerminatorKind::SwitchInt { .. }
            | mir::TerminatorKind::Return
            | mir::TerminatorKind::Unreachable => {}
            other => {
                return Err(format!(
                    "unsupported metadata terminator {other:?} in {}",
                    qualified(tcx, def)
                ))
            }
        }
    }
    // Reject loops, including effect-free divergence. Accepted declarations have
    // an acyclic CFG and a finite, acyclic helper call graph.
    fn acyclic(
        block: mir::BasicBlock,
        body: &mir::Body<'_>,
        active: &mut HashSet<mir::BasicBlock>,
        done: &mut HashSet<mir::BasicBlock>,
    ) -> bool {
        if done.contains(&block) {
            return true;
        }
        if !active.insert(block) {
            return false;
        }
        if !body.basic_blocks[block]
            .terminator()
            .successors()
            .all(|next| acyclic(next, body, active, done))
        {
            return false;
        }
        active.remove(&block);
        done.insert(block);
        true
    }
    if !acyclic(
        mir::START_BLOCK,
        body,
        &mut HashSet::new(),
        &mut HashSet::new(),
    ) {
        return Err(format!(
            "loop in metadata method/helper: {}",
            qualified(tcx, def)
        ));
    }
    active.remove(&def);
    done.insert(def);
    Ok(())
}

pub fn check(tcx: TyCtxt<'_>) {
    let mut checked = Vec::new();
    for id in tcx.mir_keys(()) {
        let def = id.to_def_id();
        if !matches!(tcx.def_kind(def), DefKind::AssocFn) {
            continue;
        }
        let Some(implementation) = tcx.trait_impl_of_assoc(def) else {
            continue;
        };
        let owner = definition_path(tcx, tcx.impl_trait_id(implementation));
        let symbol = tcx.item_name(def);
        let name = symbol.as_str();
        if !matches!(
            (owner.as_str(), name),
            ("fairing::access::mediation::protected::Permission", "gate")
                | (
                    "fairing::access::mediation::protected::PermissionBound",
                    "required_permission"
                )
        ) {
            continue;
        }
        match check_body(tcx, def, &mut HashSet::new(), &mut HashSet::new()) {
            Ok(()) => checked.push(qualified(tcx, def)),
            Err(reason) => {
                tcx.dcx().span_err(
                    tcx.def_span(def),
                    format!("FAIRING_METADATA_EFFECT: {reason}"),
                );
            }
        }
    }
    eprintln!(
        "FAIRING_METADATA_CHECKED {{\"crate\":{},\"methods\":{}}}",
        super::quote(tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str()),
        super::array(checked.iter().map(|method| super::quote(method)))
    );
    tcx.dcx().abort_if_errors();
}
