//! Compiler-owned MIR inventory for callback mediation analysis.
#![feature(rustc_private)]
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_driver::{Callbacks, Compilation};
use rustc_middle::{
    mir,
    ty::{self, TyCtxt},
};
use std::{env, fs, path::PathBuf};

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn array(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}
fn location(tcx: TyCtxt<'_>, span: rustc_middle::mir::SourceInfo) -> String {
    let loc = tcx.sess.source_map().lookup_char_pos(span.span.lo());
    format!(
        "{{\"file\":{},\"line\":{},\"column\":{}}}",
        quote(&loc.file.name.prefer_local_unconditionally().to_string()),
        loc.line,
        loc.col.0
    )
}
fn closures(tcx: TyCtxt<'_>, ty: ty::Ty<'_>, out: &mut Vec<String>) {
    match ty.kind() {
        ty::Closure(def, args) => {
            let path = tcx.def_path_str(*def);
            out.push(if def.is_local() {
                format!("{}::{path}", tcx.crate_name(def.krate))
            } else {
                path
            });
            for item in args.types() {
                closures(tcx, item, out);
            }
        }
        ty::Ref(_, inner, _) | ty::RawPtr(inner, _) | ty::Slice(inner) | ty::Array(inner, _) => {
            closures(tcx, *inner, out)
        }
        ty::Adt(_, args) => {
            for item in args.types() {
                closures(tcx, item, out);
            }
        }
        ty::FnDef(_, args) => {
            for item in args.skip_binder().types() {
                closures(tcx, item, out);
            }
        }
        ty::Tuple(items) => {
            for item in items.iter() {
                closures(tcx, item, out);
            }
        }
        _ => {}
    }
}
#[path = "metadata_driver.rs"]
mod metadata;
#[path = "mediation_schema.rs"]
mod schema;

struct Inventory {
    output: Option<PathBuf>,
}
impl Callbacks for Inventory {
    fn config(&mut self, config: &mut rustc_interface::interface::Config) {
        // Preserve the dispatch boundary in the MIR used for caller analysis.
        if self.output.is_some() || env::var("FAIRING_CHECK_METADATA").as_deref() == Ok("1") {
            config.opts.unstable_opts.inline_mir = Some(false);
        }
    }

    fn after_analysis<'tcx>(
        &mut self,
        _: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        if env::var("FAIRING_CHECK_METADATA").as_deref() == Ok("1") {
            metadata::check(tcx);
        }
        let Some(output) = &self.output else {
            return Compilation::Continue;
        };
        let mut records = Vec::new();
        let mut keys: Vec<_> = tcx.mir_keys(()).iter().copied().collect();
        keys.sort_by_key(|id| tcx.def_path_str(id.to_def_id()));
        let mut skipped = Vec::new();
        for id in &keys {
            let def = id.to_def_id();
            if !matches!(
                tcx.def_kind(def),
                rustc_hir::def::DefKind::Fn
                    | rustc_hir::def::DefKind::AssocFn
                    | rustc_hir::def::DefKind::Closure
                    | rustc_hir::def::DefKind::SyntheticCoroutineBody
            ) {
                assert!(
                    matches!(
                        tcx.def_kind(def),
                        rustc_hir::def::DefKind::Const { .. }
                            | rustc_hir::def::DefKind::AssocConst { .. }
                            | rustc_hir::def::DefKind::AnonConst
                            | rustc_hir::def::DefKind::Static { .. }
                            | rustc_hir::def::DefKind::Ctor(..)
                    ),
                    "UNSUPPORTED_MIR_BODY_KIND: {:?}",
                    tcx.def_kind(def)
                );
                skipped.push(quote(&format!(
                    "{}:{:?}",
                    tcx.def_path_str(def),
                    tcx.def_kind(def)
                )));
                continue;
            }
            let body = tcx.optimized_mir(def);
            let locals = array(body.local_decls.iter_enumerated().map(|(local, data)| {
                format!(
                    "{{\"local\":{},\"type\":{},\"shape\":{}}}",
                    local.as_usize(),
                    quote(&data.ty.to_string()),
                    schema::type_shape(tcx, data.ty)
                )
            }));
            let mut blocks = Vec::new();
            for (bb, data) in body.basic_blocks.iter_enumerated() {
                let term = data.terminator();
                let mut extra = String::new();
                let kind = match &term.kind {
                    mir::TerminatorKind::Call { func, args, .. }
                    | mir::TerminatorKind::TailCall { func, args, .. } => {
                        let ty = func.ty(&body.local_decls, tcx);
                        let mut closure_targets = Vec::new();
                        closures(tcx, ty, &mut closure_targets);
                        for arg in args {
                            closures(
                                tcx,
                                arg.node.ty(&body.local_decls, tcx),
                                &mut closure_targets,
                            );
                        }
                        closure_targets.sort();
                        closure_targets.dedup();
                        let (callee, owner, local) = if let ty::FnDef(def, _) = ty.kind() {
                            (
                                tcx.def_path_str(*def),
                                tcx.trait_of_assoc(*def).map(|t| tcx.def_path_str(t)),
                                def.is_local(),
                            )
                        } else {
                            (format!("{ty}"), None, false)
                        };
                        extra=format!(",\"callee\":{},\"trait\":{},\"local_callee\":{},\"operand\":{},\"arguments\":{}",quote(&callee),owner.as_deref().map(quote).unwrap_or_else(||"null".into()),local,quote(&format!("{func:?}")),array(args.iter().map(|arg|format!("{{\"value\":{},\"type\":{}}}",quote(&format!("{:?}",arg.node)),quote(&arg.node.ty(&body.local_decls,tcx).to_string())))));
                        if let mir::TerminatorKind::Call { destination, .. } = &term.kind {
                            extra.push_str(&format!(
                                ",\"destination_value\":{}",
                                schema::place_value(tcx, body, *destination)
                            ));
                        }

                        extra.push_str(&format!(
                            ",\"argument_values\":{}",
                            array(
                                args.iter()
                                    .map(|arg| schema::operand_value(tcx, body, &arg.node))
                            )
                        ));
                        extra.push_str(&format!(
                            ",\"closure_arguments\":{}",
                            array(closure_targets.iter().map(|s| quote(s)))
                        ));
                        if let ty::FnDef(called, _) = ty.kind() {
                            extra.push_str(&format!(
                                ",\"canonical_callee\":{},\"canonical_trait\":{}",
                                quote(&metadata::definition_path(tcx, *called)),
                                schema::protocol_of_assoc(tcx, *called)
                                    .map(|id| quote(&metadata::definition_path(tcx, id)))
                                    .unwrap_or_else(|| "null".into())
                            ));
                        }
                        extra.push_str(&format!(
                            ",\"argument_objects\":{}",
                            array(args.iter().map(|arg| schema::callable_objects(
                                tcx,
                                arg.node.ty(&body.local_decls, tcx)
                            )))
                        ));
                        if let ty::FnDef(called, generic_args) = ty.kind() {
                            extra.push_str(&format!(
                                ",\"callable_generic_parameters\":{}",
                                array(
                                    schema::callable_parameters(tcx, *called)
                                        .iter()
                                        .map(|index| index.to_string())
                                )
                            ));
                            extra.push_str(&format!(
                                ",\"generic_objects\":{}",
                                array(generic_args.skip_binder().iter().enumerate().filter_map(
                                    |(index, arg)| arg.as_type().map(|value| format!(
                                        "{{\"index\":{},\"objects\":{}}}",
                                        index,
                                        schema::callable_objects(tcx, value)
                                    ))
                                ))
                            ));
                        }

                        let owner = if let ty::FnDef(def, _) = ty.kind() {
                            tcx.trait_of_assoc(*def)
                        } else {
                            None
                        };
                        let callable = owner.is_some_and(|owner| {
                            [
                                tcx.lang_items().fn_trait(),
                                tcx.lang_items().fn_mut_trait(),
                                tcx.lang_items().fn_once_trait(),
                            ]
                            .contains(&Some(owner))
                        });
                        extra.push_str(&format!(
                            ",\"callable_trait\":{},\"local_protocol\":{},\"indirect_function\":{}",
                            callable,
                            owner.is_some_and(|d| d.is_local()
                                || matches!(
                                    tcx.crate_name(d.krate).as_str(),
                                    "fairing" | "fairing_widgets"
                                )),
                            !matches!(ty.kind(), ty::FnDef(..))
                        ));
                        "call"
                    }
                    mir::TerminatorKind::SwitchInt { discr, targets } => {
                        extra = format!(
                            ",\"discriminant\":{},\"branches\":{},\"otherwise\":{}",
                            quote(&format!("{discr:?}")),
                            array(targets.iter().map(|(value, target)| format!(
                                "{{\"value\":{},\"target\":{}}}",
                                quote(&value.to_string()),
                                target.as_usize()
                            ))),
                            targets.otherwise().as_usize()
                        );
                        "switch"
                    }
                    mir::TerminatorKind::Goto { .. } => "goto",
                    mir::TerminatorKind::Return => "return",
                    mir::TerminatorKind::Assert { .. } => "assert",
                    mir::TerminatorKind::Drop { .. } => "drop",
                    mir::TerminatorKind::UnwindResume => "unwind",
                    mir::TerminatorKind::UnwindTerminate(..) => "terminate",
                    mir::TerminatorKind::Unreachable => "unreachable",
                    _ => "unsupported",
                };
                blocks.push(format!("{{\"block\":{},\"cleanup\":{},\"statements\":{},\"kind\":{},\"successors\":{},\"location\":{},\"terminator\":{}{} }}",bb.as_usize(),data.is_cleanup,array(data.statements.iter().map(|s|quote(&format!("{:?}",s.kind)))),quote(kind),array(term.successors().map(|b|b.as_usize().to_string())),location(tcx,term.source_info),quote(&format!("{:?}",term.kind)),extra));
            }
            records.push(format!("{{\"id\":{},\"public\":{},\"closure\":{},\"argument_count\":{},\"locals\":{},\"blocks\":{},\"canonical_id\":{},\"visibility\":{},\"impl_protocol\":{},\"return_type\":{},\"bounds\":{},\"return_raw_protocols\":{},\"protocol_references\":{},\"lexical_parent\":{},\"return_parameters\":{},\"input_payload_parameters\":{},\"constructions\":{}}}",quote(&tcx.def_path_str(def)),tcx.visibility(def).is_public(),matches!(tcx.def_kind(def),rustc_hir::def::DefKind::Closure),body.arg_count,locals,array(blocks),quote(&metadata::definition_path(tcx,def)),quote(&schema::visibility(tcx,def)),schema::inherited_protocol(tcx,def).map(|v|quote(&v)).unwrap_or_else(||"null".into()),quote(&body.local_decls[mir::RETURN_PLACE].ty.to_string()),schema::bounds(tcx,def),array(schema::raw_protocols(tcx,body.local_decls[mir::RETURN_PLACE].ty).iter().map(|p|quote(p))),schema::protocol_references(tcx,body),tcx.opt_parent(def).map(|parent| quote(&format!("{}::{}",tcx.crate_name(parent.krate),tcx.def_path_str(parent)))).unwrap_or_else(||"null".into()),array(schema::raw_parameters(tcx,body.local_decls[mir::RETURN_PLACE].ty).iter().map(|p|p.to_string())),array(schema::input_payload_parameters(tcx,body).iter().map(|p|p.to_string())),schema::constructions(tcx,body)));
        }
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(
            output,
            format!(
                "{{\"crate\":{},\"mir_keys\":{},\"skipped_non_functions\":{},\"bodies\":{},\"storage_schema\":{}}}\n",
                quote(tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str()),
                keys.len(),
                array(skipped),
                array(records),
                schema::schema(tcx)
            ),
        )
        .unwrap();
        Compilation::Continue
    }
}
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    let name = args
        .windows(2)
        .find(|p| p[0] == "--crate-name")
        .map(|p| p[1].as_str());
    let output = name
        .filter(|n| matches!(*n, "fairing" | "fairing_widgets"))
        .and_then(|n| {
            env::var_os("FAIRING_MEDIATION_OUTPUT")
                .map(|path| PathBuf::from(path).join(format!("{n}.json")))
        });
    rustc_driver::run_compiler(&args, &mut Inventory { output });
}
