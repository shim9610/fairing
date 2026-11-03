//! Compiler-native state schema importer. This does not itself certify a UI abstraction.
#![feature(rustc_private)]
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_interface;
extern crate rustc_middle;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::DefKind;
use rustc_hir::def_id::{DefId, LOCAL_CRATE};
use rustc_middle::ty::{self, Ty, TyCtxt};
use std::{collections::{HashMap, HashSet}, env, fs, path::PathBuf};

fn string(value: &str) -> String {
    let mut result = String::from("\"");
    for c in value.chars() {
        match c {
            '"'=>result.push_str("\\\""), '\\'=>result.push_str("\\\\"),
            '\n'=>result.push_str("\\n"), '\r'=>result.push_str("\\r"), '\t'=>result.push_str("\\t"),
            c if c < ' '=>result.push_str(&format!("\\u{:04x}",u32::from(c))),
            c=>result.push(c),
        }
    }
    result.push('"'); result
}
fn kind(t:Ty<'_>)-> &'static str {
    // Exhaustive against the pinned compiler; a new kind must not become an opaque fallback.
    match t.kind() {
        ty::Bool=>"bool",ty::Char=>"char",ty::Int(_)=>"int",ty::Uint(_)=>"uint",ty::Float(_)=>"float",
        ty::Adt(..)=>"adt",ty::Foreign(_)=>"foreign",ty::Str=>"str",ty::Array(..)=>"array",
        ty::Pat(..)=>"pattern",ty::Slice(_)=>"slice",ty::RawPtr(..)=>"raw_pointer",
        ty::Ref(..)=>"reference",ty::FnDef(..)=>"function_definition",ty::FnPtr(..)=>"function_pointer",
        ty::UnsafeBinder(..)=>"unsafe_binder",ty::Dynamic(..)=>"dynamic",ty::Closure(..)=>"closure",
        ty::CoroutineClosure(..)=>"coroutine_closure",ty::Coroutine(..)=>"coroutine",
        ty::CoroutineWitness(..)=>"coroutine_witness",ty::Never=>"never",ty::Tuple(_)=>"tuple",
        ty::Alias(..)=>"alias",ty::Param(_)=>"parameter",ty::Bound(..)=>"bound",
        ty::Placeholder(_)=>"placeholder",ty::Infer(_)=>"inference",ty::Error(_)=>"error",
    }
}
struct Import<'tcx> {
    tcx:TyCtxt<'tcx>, ids:HashMap<Ty<'tcx>,usize>, pending:Vec<Ty<'tcx>>, records:Vec<String>, definitions:HashSet<DefId>,
}
impl<'tcx> Import<'tcx> {
    fn ty(&mut self,t:Ty<'tcx>)->usize {
        if let Some(id)=self.ids.get(&t) {return *id;}
        let id=self.pending.len();self.ids.insert(t,id);self.pending.push(t);id
    }
    fn children(&mut self,types:impl IntoIterator<Item=Ty<'tcx>>)->String {
        let values:Vec<_>=types.into_iter().map(|t|self.ty(t).to_string()).collect();
        format!("[{}]",values.join(","))
    }
    fn drain(&mut self) {
        let mut index=0;
        while index<self.pending.len() {
            let t=self.pending[index];
            let mut extra=String::new();
            match t.kind() {
                ty::Adt(def,args)=>{
                    let definition=self.tcx.def_path_str(def.did());
                    let arguments=self.children(args.types());
                    if self.definitions.insert(def.did()) {
                        let identity=ty::GenericArgs::identity_for_item(self.tcx,def.did());
                        let mut variants=Vec::new();
                        for (variant_index,v) in def.variants().iter_enumerated() {
                            let mut fields=Vec::new();
                            for (field_index,f) in v.fields.iter_enumerated() {
                                let ft=f.ty(self.tcx,identity).skip_normalization();
                                fields.push(format!("{{\"index\":{},\"name\":{},\"type\":{},\"visibility\":{}}}",
                                    field_index.as_usize(),string(f.name.as_str()),self.ty(ft),
                                    string(&format!("{:?}",self.tcx.visibility(f.did)))));
                            }
                            variants.push(format!("{{\"index\":{},\"name\":{},\"fields\":[{}]}}",
                                variant_index.as_usize(),string(v.name.as_str()),fields.join(",")));
                        }
                        self.records.push(format!("{{\"record\":\"adt_schema\",\"definition\":{},\"native_id\":{},\"union\":{},\"variants\":[{}]}}",
                            string(&definition),string(&format!("{:?}",def.did())),def.is_union(),variants.join(",")));
                    }
                    extra=format!(",\"definition\":{},\"native_id\":{},\"arguments\":{},\"all_arguments\":{}",
                        string(&definition),string(&format!("{:?}",def.did())),arguments,string(&format!("{args:?}")));
                }
                ty::Array(element,length)=>{
                    extra=format!(",\"element\":{},\"length\":{}",self.ty(*element),string(&format!("{length:?}")));
                }
                ty::Pat(element,_)|ty::Slice(element)=>{
                    extra=format!(",\"element\":{}",self.ty(*element));
                }
                ty::RawPtr(element,mutability)=>{
                    extra=format!(",\"element\":{},\"mutability\":{}",self.ty(*element),
                        string(&format!("{mutability:?}")));
                }
                ty::Ref(region,element,mutability)=>{
                    extra=format!(",\"element\":{},\"region\":{},\"mutability\":{}",self.ty(*element),
                        string(&format!("{region:?}")),string(&format!("{mutability:?}")));
                }
                ty::Tuple(types)=>{
                    extra=format!(",\"elements\":{}",self.children(types.iter()));
                }
                ty::Closure(def,args)=>{
                    let captured=ty::UpvarArgs::Closure(args).upvar_tys().iter();
                    extra=format!(",\"definition\":{},\"captures\":{}",
                        string(&self.tcx.def_path_str(*def)),self.children(captured));
                }
                ty::CoroutineClosure(def,args)=>{
                    let captured=ty::UpvarArgs::CoroutineClosure(args).upvar_tys().iter();
                    extra=format!(",\"definition\":{},\"captures\":{}",
                        string(&self.tcx.def_path_str(*def)),self.children(captured));
                }
                ty::Coroutine(def,args)=>{
                    let captured=ty::UpvarArgs::Coroutine(args).upvar_tys().iter();
                    extra=format!(",\"definition\":{},\"captures\":{},\"suspended_storage\":\"unproved\"",
                        string(&self.tcx.def_path_str(*def)),self.children(captured));
                }
                ty::CoroutineWitness(def,args)=>{
                    extra=format!(",\"definition\":{},\"arguments\":{},\"witness_storage\":\"unproved\"",
                        string(&self.tcx.def_path_str(*def)),self.children(args.types()));
                }
                ty::FnDef(def,args)=>{
                    extra=format!(",\"definition\":{},\"arguments\":{}",
                        string(&self.tcx.def_path_str(*def)),self.children(args.skip_binder().types()));
                }
                ty::Alias(_,alias)=>{
                    extra=format!(",\"alias_arguments\":{},\"normalization\":\"unproved\"",
                        self.children(alias.args.types()));
                }
                // These have no recursively owned aggregate fields. Pointer referents and
                // runtime-erased payloads require a separate heap relation, never an implicit World.
                ty::Bool|ty::Char|ty::Int(_)|ty::Uint(_)|ty::Float(_)|ty::Foreign(_)|ty::Str
                |ty::FnPtr(..)|ty::UnsafeBinder(..)|ty::Dynamic(..)|ty::Never|ty::Param(_)
                |ty::Bound(..)|ty::Placeholder(_)|ty::Infer(_)|ty::Error(_)=>{}
            }
            self.records.push(format!("{{\"record\":\"type\",\"id\":{},\"kind\":{},\"native\":{}{}}}",
                index,string(kind(t)),string(&format!("{t:?}")),extra));
            index+=1;
        }
    }
}
struct StateTypes;
impl Callbacks for StateTypes {
    fn after_analysis<'tcx>(&mut self,_compiler:&rustc_interface::interface::Compiler,tcx:TyCtxt<'tcx>)->Compilation {
        let name=tcx.crate_name(LOCAL_CRATE).to_string();
        if !["fairing","fairing_widgets","egui"].contains(&name.as_str()) {return Compilation::Continue;}
        let mut imported=Import {tcx,ids:HashMap::new(),pending:Vec::new(),records:Vec::new(),definitions:HashSet::new()};
        let mut roots=Vec::new();
        for local in tcx.iter_local_def_id() {
            if matches!(tcx.def_kind(local),DefKind::Struct|DefKind::Enum|DefKind::Union|DefKind::TyAlias|DefKind::Static{..}) {
                let ty=tcx.type_of(local).instantiate_identity().skip_normalization();
                roots.push(format!("{{\"record\":\"declaration\",\"definition\":{},\"type\":{}}}",
                    string(&tcx.def_path_str(local)),imported.ty(ty)));
            }
        }
        for local in tcx.hir_body_owners() {
            let def=local.to_def_id();
            let body=match tcx.def_kind(local) {
                DefKind::Fn|DefKind::AssocFn|DefKind::Closure=>tcx.optimized_mir(def),
                _=>tcx.mir_for_ctfe(def),
            };
            let mut locals=Vec::new();
            for (slot,decl) in body.local_decls.iter_enumerated() {
                locals.push(format!("{{\"slot\":{},\"type\":{}}}",slot.as_usize(),imported.ty(decl.ty)));
            }
            let blocks:Vec<_>=body.basic_blocks.iter().enumerate()
                .map(|(i,b)|format!("{{\"block\":{},\"statement_locations\":{}}}",i,b.statements.len()+1)).collect();
            roots.push(format!("{{\"record\":\"body\",\"definition\":{},\"visibility\":{},\"arguments\":{},\"locals\":[{}],\"locations\":[{}]}}",
                string(&tcx.def_path_str(def)),string(&format!("{:?}",tcx.visibility(def))),body.arg_count,locals.join(","),blocks.join(",")));
        }
        imported.drain();
        let destination=PathBuf::from(env::var_os("FAIRING_STATE_TYPES_OUT").expect("state import output"));
        fs::create_dir_all(&destination).expect("create state import directory");
        let content=roots.into_iter().chain(imported.records).collect::<Vec<_>>().join("\n")+"\n";
        fs::write(destination.join(format!("{name}.jsonl")),content).expect("write state types");
        Compilation::Continue
    }
}
fn main() {
    let mut args:Vec<_>=env::args().skip(1).collect();
    if args.is_empty() {args.push("rustc".to_owned());}
    rustc_driver::compiler_entrypoint(&args,&mut StateTypes);
}
