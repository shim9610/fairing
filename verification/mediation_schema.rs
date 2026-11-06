//! Compiler-owned carrier/escape schema; names are definition identities, not aliases.
use rustc_hir::def::DefKind;
use rustc_middle::ty::{self, TyCtxt};

pub fn protocol_of_assoc(
    tcx: TyCtxt<'_>,
    def: rustc_hir::def_id::DefId,
) -> Option<rustc_hir::def_id::DefId> {
    tcx.trait_of_assoc(def)
        .or_else(|| tcx.trait_impl_of_assoc(def).map(|id| tcx.impl_trait_id(id)))
}

fn carrier(tcx: TyCtxt<'_>, def: rustc_hir::def_id::DefId) -> bool {
    matches!(
        identity(tcx, def).as_str(),
        "fairing::access::mediation::Callback"
            | "fairing::access::mediation::ScreenHandle"
            | "fairing::access::mediation::FactoryCallback"
            | "fairing::access::mediation::element::ElementHandle"
            | "fairing::access::mediation::protected::Protected"
    )
}

// Unlike Ty::walk, stop at opaque carrier boundaries. A Callback<dyn FnMut>
// field is a protected handle, not exposed callable storage.
pub fn raw_protocols(tcx: TyCtxt<'_>, value: ty::Ty<'_>) -> Vec<String> {
    let mut pending = vec![value];
    let mut found = Vec::new();
    while let Some(value) = pending.pop() {
        match value.kind() {
            ty::Adt(def, _) if carrier(tcx, def.did()) => {}
            ty::Adt(_, args) | ty::Closure(_, args) => pending.extend(args.types()),
            ty::Ref(_, inner, _)
            | ty::RawPtr(inner, _)
            | ty::Slice(inner)
            | ty::Array(inner, _) => pending.push(*inner),
            ty::Tuple(items) => pending.extend(items.iter()),
            ty::Dynamic(..) | ty::FnPtr(..) => found.extend(protocols(tcx, value)),
            _ => {}
        }
    }
    found.sort();
    found.dedup();
    found
}

pub fn raw_parameters(tcx: TyCtxt<'_>, value: ty::Ty<'_>) -> Vec<u32> {
    let mut pending = vec![value];
    let mut found = Vec::new();
    while let Some(value) = pending.pop() {
        match value.kind() {
            ty::Adt(def, _) if carrier(tcx, def.did()) => {}
            ty::Adt(_, args) => pending.extend(args.types()),
            ty::Ref(_, inner, _)
            | ty::RawPtr(inner, _)
            | ty::Slice(inner)
            | ty::Array(inner, _) => pending.push(*inner),
            ty::Tuple(items) => pending.extend(items.iter()),
            ty::Param(parameter) => found.push(parameter.index),
            _ => {}
        }
    }
    found.sort();
    found.dedup();
    found
}

pub fn input_payload_parameters<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
) -> Vec<u32> {
    let mut found = Vec::new();
    for local in body.args_iter() {
        for arg in body.local_decls[local].ty.walk() {
            let Some(value) = arg.as_type() else {
                continue;
            };
            if let ty::Adt(def, args) = value.kind() {
                if carrier(tcx, def.did()) {
                    for field in def
                        .all_fields()
                        .filter(|field| field.name.as_str() == "body")
                    {
                        found.extend(raw_parameters(tcx, field.ty(tcx, args).skip_norm_wip()));
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

pub fn protocol_references<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
) -> String {
    use rustc_middle::mir::{self, visit::Visitor};
    struct References<'tcx, 'a> {
        tcx: TyCtxt<'tcx>,
        body: &'a mir::Body<'tcx>,
        found: Vec<(String, String)>,
    }
    impl<'tcx> Visitor<'tcx> for References<'tcx, '_> {
        fn visit_operand(&mut self, operand: &mir::Operand<'tcx>, location: mir::Location) {
            for arg in operand.ty(&self.body.local_decls, self.tcx).walk() {
                let Some(value) = arg.as_type() else {
                    continue;
                };
                if let ty::FnDef(def, _) = value.kind() {
                    if let Some(protocol) = protocol_of_assoc(self.tcx, *def) {
                        self.found
                            .push((identity(self.tcx, *def), identity(self.tcx, protocol)));
                    }
                }
            }
            self.super_operand(operand, location);
        }
    }
    let mut visitor = References {
        tcx,
        body,
        found: Vec::new(),
    };
    visitor.visit_body(body);
    visitor.found.sort();
    visitor.found.dedup();
    super::array(visitor.found.iter().map(|(id, protocol)| {
        format!(
            "{{\"id\":{},\"protocol\":{}}}",
            super::quote(id),
            super::quote(protocol)
        )
    }))
}

fn identity(tcx: TyCtxt<'_>, def: rustc_hir::def_id::DefId) -> String {
    super::metadata::definition_path(tcx, def)
}
fn protocols(tcx: TyCtxt<'_>, value: ty::Ty<'_>) -> Vec<String> {
    let mut found = Vec::new();
    for argument in value.walk() {
        let Some(value) = argument.as_type() else {
            continue;
        };
        match value.kind() {
            ty::Dynamic(predicates, _) => {
                for predicate in predicates.iter() {
                    if let ty::ExistentialPredicate::Trait(trait_ref) = predicate.skip_binder() {
                        found.push(identity(tcx, trait_ref.def_id));
                    }
                }
            }
            ty::FnPtr(..) => found.push("function-pointer".into()),
            ty::Closure(def, _) => found.push(identity(tcx, *def)),
            _ => {}
        }
    }
    found.sort();
    found.dedup();
    found
}
pub fn schema(tcx: TyCtxt<'_>) -> String {
    let mut records = Vec::new();
    for local in tcx.iter_local_def_id() {
        let def = local.to_def_id();
        if !matches!(
            tcx.def_kind(def),
            DefKind::Struct | DefKind::Enum | DefKind::Union
        ) {
            continue;
        }
        let ty = tcx.type_of(def).instantiate_identity().skip_norm_wip();
        let ty::Adt(adt, args) = ty.kind() else {
            continue;
        };
        let fields = adt.all_fields().map(|field| {
            let value = field.ty(tcx, args).skip_norm_wip();
            format!(
                "{{\"id\":{},\"type\":{},\"visibility\":{},\"protocols\":{},\"raw_protocols\":{}}}",
                super::quote(&identity(tcx, field.did)),
                super::quote(&value.to_string()),
                super::quote(&visibility(tcx, field.did)),
                super::array(protocols(tcx, value).iter().map(|p| super::quote(p))),
                super::array(raw_protocols(tcx, value).iter().map(|p| super::quote(p)))
            )
        });
        records.push(format!(
            "{{\"id\":{},\"visibility\":{},\"fields\":{}}}",
            super::quote(&identity(tcx, def)),
            super::quote(&visibility(tcx, def)),
            super::array(fields)
        ));
    }
    super::array(records)
}

pub fn visibility(tcx: TyCtxt<'_>, def: rustc_hir::def_id::DefId) -> String {
    match tcx.visibility(def) {
        ty::Visibility::Public => "public".into(),
        ty::Visibility::Restricted(scope) => identity(tcx, scope.into()),
    }
}
pub fn inherited_protocol(tcx: TyCtxt<'_>, def: rustc_hir::def_id::DefId) -> Option<String> {
    let mut current = Some(def);
    while let Some(def) = current {
        if matches!(tcx.def_kind(def), DefKind::AssocFn) {
            if let Some(protocol) = protocol_of_assoc(tcx, def) {
                return Some(identity(tcx, protocol));
            }
        }
        current = tcx.opt_parent(def);
    }
    None
}
pub fn bounds(tcx: TyCtxt<'_>, def: rustc_hir::def_id::DefId) -> String {
    super::array(
        tcx.param_env(def)
            .caller_bounds()
            .iter()
            .filter_map(|clause| {
                let clause = clause.as_trait_clause()?;
                Some(format!(
                    "{{\"self_type\":{},\"trait\":{},\"predicate\":{}}}",
                    super::quote(&clause.self_ty().skip_binder().to_string()),
                    super::quote(&identity(tcx, clause.def_id())),
                    super::quote(&format!("{clause:?}"))
                ))
            }),
    )
}
pub fn callable_objects(tcx: TyCtxt<'_>, ty: ty::Ty<'_>) -> String {
    let mut value = ty;
    loop {
        let record = match value.kind() {
            ty::Ref(_, inner, _) => {
                value = *inner;
                continue;
            }
            ty::Adt(_, args) => {
                let mut objects = Vec::new();
                for item in args.types() {
                    let encoded = callable_objects(tcx, item);
                    if encoded != "[]" {
                        objects.push(encoded[1..encoded.len() - 1].to_owned());
                    }
                }
                return format!("[{}]", objects.join(","));
            }
            ty::Tuple(items) => {
                let mut objects = Vec::new();
                for item in items.iter() {
                    let encoded = callable_objects(tcx, item);
                    if encoded != "[]" {
                        objects.push(encoded[1..encoded.len() - 1].to_owned());
                    }
                }
                return format!("[{}]", objects.join(","));
            }
            ty::Closure(def, _) | ty::FnDef(def, _) => {
                let path = tcx.def_path_str(*def);
                let path = if def.is_local() {
                    format!("{}::{path}", tcx.crate_name(def.krate))
                } else {
                    path
                };
                format!(
                    "{{\"kind\":\"definition\",\"id\":{},\"local\":{},\"protocol\":{}}}",
                    super::quote(&path),
                    def.is_local(),
                    protocol_of_assoc(tcx, *def)
                        .map(|p| super::quote(&identity(tcx, p)))
                        .unwrap_or_else(|| "null".into())
                )
            }
            ty::Param(parameter) => format!(
                "{{\"kind\":\"parameter\",\"index\":{},\"name\":{}}}",
                parameter.index,
                super::quote(parameter.name.as_str())
            ),
            ty::Dynamic(..) | ty::FnPtr(..) => "{\"kind\":\"dynamic\"}".into(),
            _ => return "[]".into(),
        };
        return format!("[{record}]");
    }
}

pub fn constructions<'tcx>(tcx: TyCtxt<'tcx>, body: &rustc_middle::mir::Body<'tcx>) -> String {
    use rustc_middle::mir::{self, Rvalue, StatementKind};
    let mut found = Vec::new();
    for (bb, block) in body.basic_blocks.iter_enumerated() {
        for statement in &block.statements {
            let StatementKind::Assign(assignment) = &statement.kind else {
                continue;
            };
            let destination = &assignment.0;
            let written_fields = destination
                .iter_projections()
                .filter_map(|(base, projection)| {
                    let mir::ProjectionElem::Field(index, _) = projection else {
                        return None;
                    };
                    let ty::Adt(def, _) = base.ty(&body.local_decls, tcx).ty.kind() else {
                        return None;
                    };
                    def.is_struct()
                        .then(|| identity(tcx, def.non_enum_variant().fields[index].did))
                });
            let (kind, values, owner, variant) = match &assignment.1 {
                Rvalue::Use(operand, _) | Rvalue::Cast(_, operand, _) => {
                    ("use", vec![format!("{operand:?}")], None, None)
                }
                Rvalue::Ref(_, _, place) | Rvalue::CopyForDeref(place) => {
                    ("reference", vec![format!("{place:?}")], None, None)
                }
                Rvalue::Aggregate(kind, operands) => {
                    let (owner, variant) = match &**kind {
                        mir::AggregateKind::Adt(def, index, ..) => {
                            let adt = tcx.adt_def(*def);
                            (
                                Some(identity(tcx, *def)),
                                Some(adt.variant(*index).name.to_string()),
                            )
                        }
                        _ => (None, None),
                    };
                    (
                        "aggregate",
                        operands.iter().map(|value| format!("{value:?}")).collect(),
                        owner,
                        variant,
                    )
                }
                _ => ("other", Vec::new(), None, None),
            };
            let mut fields = Vec::new();
            if let Rvalue::Aggregate(kind, operands) = &assignment.1 {
                if let mir::AggregateKind::Adt(def, index, ..) = &**kind {
                    for (field, operand) in tcx
                        .adt_def(*def)
                        .variant(*index)
                        .fields
                        .iter()
                        .zip(operands.iter())
                    {
                        fields.push(format!(
                            "{{\"field\":{},\"value\":{}}}",
                            super::quote(&identity(tcx, field.did)),
                            super::quote(&format!("{operand:?}"))
                        ));
                    }
                }
            }
            found.push(format!("{{\"block\":{},\"destination\":{},\"kind\":{},\"values\":{},\"owner\":{},\"variant\":{},\"fields\":{},\"written_fields\":{}}}",
                bb.as_usize(),super::quote(&format!("{destination:?}")),super::quote(kind),super::array(values.iter().map(|v|super::quote(v))),owner.as_deref().map(super::quote).unwrap_or_else(||"null".into()),variant.as_deref().map(super::quote).unwrap_or_else(||"null".into()),super::array(fields),super::array(written_fields.map(|field|super::quote(&field)))));
        }
    }
    super::array(found)
}

pub fn callable_parameters(tcx: TyCtxt<'_>, def: rustc_hir::def_id::DefId) -> Vec<u32> {
    tcx.param_env(def)
        .caller_bounds()
        .iter()
        .filter_map(|clause| {
            let clause = clause.as_trait_clause()?;
            if ![
                tcx.lang_items().fn_trait(),
                tcx.lang_items().fn_mut_trait(),
                tcx.lang_items().fn_once_trait(),
            ]
            .contains(&Some(clause.def_id()))
            {
                return None;
            }
            let ty::Param(parameter) = clause.self_ty().skip_binder().kind() else {
                return None;
            };
            Some(parameter.index)
        })
        .collect()
}
