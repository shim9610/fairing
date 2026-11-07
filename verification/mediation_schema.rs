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

#[derive(Clone)]
struct Contract {
    owner: rustc_hir::def_id::DefId,
    role: String,
    protocols: Vec<String>,
    generated: bool,
    parameters: Vec<u32>,
}
thread_local! {
    static CONTRACTS: std::cell::RefCell<Option<Vec<Contract>>> = const { std::cell::RefCell::new(None) };
}
fn declarations(tcx: TyCtxt<'_>) -> Vec<Contract> {
    CONTRACTS.with(|cache| {
        if let Some(items) = &*cache.borrow() {
            return items.clone();
        }
        let mut records = Vec::new();
        for (&trait_id, implementations) in tcx.all_local_trait_impls(()) {
            let path = identity(tcx, trait_id);
            let role = match path.as_str() {
                "fairing::access::mediation::contract::Carrier" => "carrier",
                "fairing::access::mediation::contract::Declaration" => "declaration",
                _ => continue,
            };
            let contract_module = tcx.parent(trait_id);
            let generator = tcx
                .iter_local_def_id()
                .map(|id| id.to_def_id())
                .find(|&def| {
                    matches!(tcx.def_kind(def), DefKind::Macro(..))
                        && tcx.opt_parent(def) == Some(contract_module)
                        && tcx.item_name(def).as_str() == "protected_contract"
                });
            for implementation in implementations {
                let implementation = implementation.to_def_id();
                let self_ty = tcx
                    .type_of(implementation)
                    .instantiate_identity()
                    .skip_norm_wip();
                let ty::Adt(owner, _) = self_ty.kind() else {
                    panic!("Non-ADT mediation contract");
                };
                let payload = tcx
                    .associated_items(implementation)
                    .in_definition_order()
                    .find(|item| {
                        matches!(item.kind, ty::AssocKind::Type { .. })
                            && item.name().as_str() == "Payload"
                    })
                    .map(|item| {
                        tcx.type_of(item.def_id)
                            .instantiate_identity()
                            .skip_norm_wip()
                    });
                let generated = generator.is_some()
                    && tcx
                        .def_span(implementation)
                        .ctxt()
                        .outer_expn_data()
                        .macro_def_id
                        == generator;
                records.push(Contract {
                    owner: owner.did(),
                    role: role.to_owned(),
                    protocols: payload.map(|ty| protocols(tcx, ty)).unwrap_or_default(),
                    parameters: payload
                        .map(|value| raw_parameters_in(tcx, value, false))
                        .unwrap_or_default(),
                    generated,
                });
            }
        }
        *cache.borrow_mut() = Some(records.clone());
        records
    })
}
fn carrier(tcx: TyCtxt<'_>, def: rustc_hir::def_id::DefId) -> bool {
    declarations(tcx)
        .iter()
        .any(|entry| entry.role == "carrier" && entry.owner == def)
}
pub fn contracts(tcx: TyCtxt<'_>) -> String {
    super::array(declarations(tcx).iter().map(|entry| {
        format!(
            "{{\"owner\":{},\"role\":{},\"protocols\":{},\"generated\":{}}}",
            super::quote(&identity(tcx, entry.owner)),
            super::quote(&entry.role),
            super::array(entry.protocols.iter().map(|p| super::quote(p))),
            entry.generated
        )
    }))
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

fn raw_parameters_in(tcx: TyCtxt<'_>, value: ty::Ty<'_>, stop_at_carrier: bool) -> Vec<u32> {
    let mut pending = vec![value];
    let mut found = Vec::new();
    while let Some(value) = pending.pop() {
        match value.kind() {
            ty::Adt(def, _) if stop_at_carrier && carrier(tcx, def.did()) => {}
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

pub fn raw_parameters(tcx: TyCtxt<'_>, value: ty::Ty<'_>) -> Vec<u32> {
    raw_parameters_in(tcx, value, true)
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
                for declaration in declarations(tcx)
                    .iter()
                    .filter(|entry| entry.role == "carrier" && entry.owner == def.did())
                {
                    for index in &declaration.parameters {
                        if let Some(argument) =
                            args.get(*index as usize).and_then(|arg| arg.as_type())
                        {
                            found.extend(raw_parameters(tcx, argument));
                        }
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

pub fn type_shape(tcx: TyCtxt<'_>, value: ty::Ty<'_>) -> String {
    match value.kind() {
        ty::Pat(inner, _) => type_shape(tcx, *inner),
        ty::Adt(def, args) => format!(
            "{{\"kind\":\"adt\",\"id\":{},\"arguments\":{}}}",
            super::quote(&identity(tcx, def.did())),
            super::array(args.iter().enumerate().filter_map(|(index, arg)| {
                arg.as_type()
                    .map(|ty| format!("{{\"index\":{},\"value\":{}}}", index, type_shape(tcx, ty)))
            }))
        ),
        ty::Ref(_, inner, mutability) => format!(
            "{{\"kind\":\"reference\",\"mutable\":{},\"value\":{}}}",
            mutability.is_mut(),
            type_shape(tcx, *inner)
        ),
        ty::RawPtr(inner, _) => format!(
            "{{\"kind\":\"pointer\",\"value\":{}}}",
            type_shape(tcx, *inner)
        ),
        ty::Slice(inner) | ty::Array(inner, _) => format!(
            "{{\"kind\":\"sequence\",\"value\":{}}}",
            type_shape(tcx, *inner)
        ),
        ty::Tuple(items) => format!(
            "{{\"kind\":\"tuple\",\"values\":{}}}",
            super::array(items.iter().map(|ty| type_shape(tcx, ty)))
        ),
        ty::Param(parameter) => format!("{{\"kind\":\"parameter\",\"index\":{}}}", parameter.index),
        ty::Alias(_, alias) if alias.kind.try_to_projection().is_some() => format!(
            "{{\"kind\":\"projection\",\"id\":{},\"values\":{}}}",
            super::quote(&identity(
                tcx,
                alias.kind.try_to_projection().unwrap().into()
            )),
            super::array(alias.args.types().map(|ty| type_shape(tcx, ty)))
        ),
        ty::Dynamic(..) => format!(
            "{{\"kind\":\"dynamic\",\"protocols\":{}}}",
            super::array(protocols(tcx, value).iter().map(|id| super::quote(id)))
        ),
        ty::FnPtr(..) => "{\"kind\":\"callable\"}".into(),
        ty::FnDef(..) => "{\"kind\":\"scalar\"}".into(),
        ty::Closure(..) => "{\"kind\":\"closure-data\"}".into(),
        ty::Bool | ty::Char | ty::Int(..) | ty::Uint(..) | ty::Float(..) | ty::Str | ty::Never => {
            "{\"kind\":\"scalar\"}".into()
        }
        _ => format!(
            "{{\"kind\":\"unresolved\",\"type\":{}}}",
            super::quote(&value.to_string())
        ),
    }
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
    let mut pending: Vec<_> = tcx.iter_local_def_id().map(|id| id.to_def_id()).collect();
    let mut done = std::collections::HashSet::new();
    while let Some(def) = pending.pop() {
        if !done.insert(def) {
            continue;
        }
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
            for argument in value.walk() {
                if let Some(value) = argument.as_type() {
                    if let ty::Adt(dependency,_) = value.kind() { pending.push(dependency.did()); }
                }
            }
            format!(
                "{{\"id\":{},\"type\":{},\"visibility\":{},\"protocols\":{},\"raw_protocols\":{},\"shape\":{}}}",
                super::quote(&identity(tcx, field.did)),
                super::quote(&value.to_string()),
                super::quote(&visibility(tcx, field.did)),
                super::array(protocols(tcx, value).iter().map(|p| super::quote(p))),
                super::array(raw_protocols(tcx, value).iter().map(|p| super::quote(p))),type_shape(tcx,value)
            )
        });
        records.push(format!(
            "{{\"id\":{},\"visibility\":{},\"local\":{},\"fields\":{}}}",
            super::quote(&identity(tcx, def)),
            super::quote(&visibility(tcx, def)),
            def.is_local(),
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
            let operand_facts = match &assignment.1 {
                Rvalue::Use(operand, _) | Rvalue::Cast(_, operand, _) => {
                    vec![operand_value(tcx, body, operand)]
                }
                Rvalue::Ref(_, _, place) | Rvalue::CopyForDeref(place) => {
                    vec![place_value(tcx, body, *place)]
                }
                Rvalue::Aggregate(_, operands) => operands
                    .iter()
                    .map(|operand| operand_value(tcx, body, operand))
                    .collect(),
                _ => Vec::new(),
            };
            let closure = if let Rvalue::Aggregate(kind, _) = &assignment.1 {
                if let mir::AggregateKind::Closure(def, _) = &**kind {
                    Some(format!(
                        "{}::{}",
                        tcx.crate_name(def.krate),
                        tcx.def_path_str(*def)
                    ))
                } else {
                    None
                }
            } else {
                None
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
            found.push(format!("{{\"block\":{},\"destination\":{},\"kind\":{},\"values\":{},\"owner\":{},\"variant\":{},\"fields\":{},\"written_fields\":{},\"operand_facts\":{},\"destination_value\":{},\"closure\":{}}}",
                bb.as_usize(),super::quote(&format!("{destination:?}")),super::quote(kind),super::array(values.iter().map(|v|super::quote(v))),owner.as_deref().map(super::quote).unwrap_or_else(||"null".into()),variant.as_deref().map(super::quote).unwrap_or_else(||"null".into()),super::array(fields),super::array(written_fields.map(|field|super::quote(&field))),super::array(operand_facts),place_value(tcx,body,*destination),closure.as_deref().map(super::quote).unwrap_or_else(||"null".into())));
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

pub fn place_value<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    place: rustc_middle::mir::Place<'tcx>,
) -> String {
    use rustc_middle::mir::ProjectionElem;
    let projections = place
        .iter_projections()
        .map(|(base, projection)| match projection {
            ProjectionElem::Deref => "{\"kind\":\"deref\"}".to_owned(),
            ProjectionElem::Field(index, _) => {
                let base_ty = base.ty(&body.local_decls, tcx);
                let field = match base_ty.ty.kind() {
                    ty::Adt(def, _) => Some(identity(
                        tcx,
                        base_ty
                            .variant_index
                            .map(|variant| def.variant(variant).fields[index].did)
                            .unwrap_or_else(|| def.non_enum_variant().fields[index].did),
                    )),
                    _ => None,
                };
                format!(
                    "{{\"kind\":\"field\",\"index\":{},\"id\":{}}}",
                    index.as_usize(),
                    field
                        .as_deref()
                        .map(super::quote)
                        .unwrap_or_else(|| "null".into())
                )
            }
            ProjectionElem::Downcast(_, index) => {
                format!("{{\"kind\":\"downcast\",\"index\":{}}}", index.as_usize())
            }
            ProjectionElem::Index(..) | ProjectionElem::ConstantIndex { .. } => {
                "{\"kind\":\"index\"}".into()
            }
            _ => "{\"kind\":\"other\"}".into(),
        });
    format!(
        "{{\"kind\":\"place\",\"local\":{},\"projections\":{},\"shape\":{}}}",
        place.local.as_usize(),
        super::array(projections),
        type_shape(tcx, place.ty(&body.local_decls, tcx).ty)
    )
}

pub fn operand_value<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &rustc_middle::mir::Body<'tcx>,
    operand: &rustc_middle::mir::Operand<'tcx>,
) -> String {
    match operand {
        rustc_middle::mir::Operand::Copy(place) | rustc_middle::mir::Operand::Move(place) => {
            place_value(tcx, body, *place)
        }
        rustc_middle::mir::Operand::Constant(..) => format!(
            "{{\"kind\":\"constant\",\"objects\":{},\"literal\":{},\"shape\":{}}}",
            callable_objects(tcx, operand.ty(&body.local_decls, tcx)),
            super::quote(&format!("{operand:?}")),
            type_shape(tcx, operand.ty(&body.local_decls, tcx))
        ),
        _ => "{\"kind\":\"unknown\"}".into(),
    }
}

pub fn impl_owner(tcx: TyCtxt<'_>, mut def: rustc_hir::def_id::DefId) -> Option<String> {
    while let Some(parent) = tcx.opt_parent(def) {
        if matches!(tcx.def_kind(parent), DefKind::Impl { .. }) {
            let value = tcx.type_of(parent).instantiate_identity().skip_norm_wip();
            return if let ty::Adt(owner, _) = value.kind() {
                Some(identity(tcx, owner.did()))
            } else {
                None
            };
        }
        def = parent;
    }
    None
}

pub fn protected_protocols<'tcx>(
    tcx: TyCtxt<'tcx>,
    value: ty::Ty<'tcx>,
    def: rustc_hir::def_id::DefId,
) -> Vec<String> {
    let mut found = Vec::new();
    for argument in value.walk() {
        let Some(value) = argument.as_type() else {
            continue;
        };
        let ty::Adt(owner, args) = value.kind() else {
            continue;
        };
        for contract in declarations(tcx)
            .iter()
            .filter(|c| c.role == "carrier" && c.owner == owner.did())
        {
            found.extend(contract.protocols.iter().cloned());
            for index in &contract.parameters {
                if let Some(payload) = args.get(*index as usize).and_then(|a| a.as_type()) {
                    found.extend(protocols(tcx, payload));
                    if matches!(payload.kind(), ty::Param(_)) {
                        for clause in tcx.param_env(def).caller_bounds() {
                            let Some(bound) = clause.as_trait_clause() else {
                                continue;
                            };
                            if bound.self_ty().skip_binder() != payload {
                                continue;
                            }
                            let protocol = bound.def_id();
                            let path = identity(tcx, protocol);
                            if !matches!(
                                tcx.crate_name(protocol.krate).as_str(),
                                "core" | "std" | "alloc"
                            ) && !matches!(
                                path.as_str(),
                                "fairing::access::mediation::protected::Permission"
                                    | "fairing::access::mediation::protected::PermissionBound"
                                    | "fairing::access::mediation::protected::ProtectedDeclaration"
                                    | "fairing::access::mediation::protected::sealed::Declaration"
                                    | "fairing::access::mediation::contract::Declaration"
                            ) {
                                found.push(path);
                            }
                        }
                    }
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}
