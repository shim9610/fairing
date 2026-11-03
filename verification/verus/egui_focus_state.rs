//! Concrete egui focus-domain specification. No application-wide claim.
use vstd::prelude::*;

verus! {
pub type Id = u64;
pub enum Direction { None, Up, Right, Down, Left, Next, Previous }
// Field domains retain the complete native filter, layer and rectangle values.
pub struct Owner<Filter> { pub id:Id, pub filter:Filter }
pub struct NativeFocus<Filter,Layer,Rect> {
    pub focused_widget:Option<Owner<Filter>>,
    pub id_previous_frame:Option<Id>,
    pub id_two_frames_ago:Option<Id>,
    pub id_next_frame:Option<Id>,
    pub give_to_next:bool,
    pub last_interested:Option<Id>,
    pub focus_direction:Direction,
    pub top_modal_layer:Option<Layer>,
    pub top_modal_layer_current_frame:Option<Layer>,
    pub focus_widgets_cache:Map<Id,Rect>,
}
pub struct History {pub previous:Option<Id>,pub two_passes_ago:Option<Id>}
pub struct Modal<Layer> {pub previous:Option<Layer>,pub current:Option<Layer>}
pub struct Navigation<Filter,Layer,Rect> {
    pub current:Option<Owner<Filter>>,
    pub history:History,
    pub deferred:Option<Id>,
    pub next_interested_gets_focus:bool,
    pub last_interested:Option<Id>,
    pub direction:Direction,
    pub modal:Modal<Layer>,
    pub candidates:Map<Id,Rect>,
}
pub open spec fn encode<F,L,R>(s:NativeFocus<F,L,R>)->Navigation<F,L,R> {
    Navigation {
        current:s.focused_widget,
        history:History {previous:s.id_previous_frame,two_passes_ago:s.id_two_frames_ago},
        deferred:s.id_next_frame,next_interested_gets_focus:s.give_to_next,
        last_interested:s.last_interested,direction:s.focus_direction,
        modal:Modal {previous:s.top_modal_layer,current:s.top_modal_layer_current_frame},
        candidates:s.focus_widgets_cache,
    }
}
pub open spec fn decode<F,L,R>(s:Navigation<F,L,R>)->NativeFocus<F,L,R> {
    NativeFocus {
        focused_widget:s.current,id_previous_frame:s.history.previous,
        id_two_frames_ago:s.history.two_passes_ago,id_next_frame:s.deferred,
        give_to_next:s.next_interested_gets_focus,last_interested:s.last_interested,
        focus_direction:s.direction,top_modal_layer:s.modal.previous,
        top_modal_layer_current_frame:s.modal.current,focus_widgets_cache:s.candidates,
    }
}
pub open spec fn native_focused<F,L,R>(s:NativeFocus<F,L,R>)->Option<Id> {
    match s.focused_widget {None=>None,Some(owner)=>Some(owner.id)}
}
pub open spec fn specified_focused<F,L,R>(s:Navigation<F,L,R>)->Option<Id> {
    match s.current {None=>None,Some(owner)=>Some(owner.id)}
}
pub enum Current {Unowned, Owned}
pub enum Deferred {Absent, SameOwner, OtherOwner}
pub struct Group {pub current:Current,pub deferred:Deferred,pub next_interested:bool,pub direction:Direction}
pub open spec fn classify<F,L,R>(s:Navigation<F,L,R>)->Group {
    Group {
        current:if s.current.is_some(){Current::Owned}else{Current::Unowned},
        deferred:match s.deferred {
            None=>Deferred::Absent,
            Some(id)=>if specified_focused(s)==Some(id){Deferred::SameOwner}else{Deferred::OtherOwner},
        },
        next_interested:s.next_interested_gets_focus,direction:s.direction,
    }
}
pub open spec fn member<F,L,R>(s:Navigation<F,L,R>,g:Group)->bool {
    (match g.current {Current::Unowned=>s.current.is_none(),Current::Owned=>s.current.is_some()})
    && (match g.deferred {
        Deferred::Absent=>s.deferred.is_none(),
        Deferred::SameOwner=>s.deferred.is_some() && s.deferred==specified_focused(s),
        Deferred::OtherOwner=>s.deferred.is_some() && s.deferred!=specified_focused(s),
    }) && g.next_interested==s.next_interested_gets_focus && g.direction==s.direction
}
pub proof fn complete_native_focus_domain<F,L,R>(s:NativeFocus<F,L,R>)
    ensures decode(encode(s))==s,
        native_focused(s)==specified_focused(encode(s)),
        member(encode(s),classify(encode(s))),
        forall |g:Group| member(encode(s),g) ==> g==classify(encode(s))
{}
pub proof fn canonical_focus_domain<F,L,R>(s:Navigation<F,L,R>)
    ensures encode(decode(s))==s
{}
pub proof fn no_history_or_filter_is_erased<F,L,R,O>(s:NativeFocus<F,L,R>,observe:spec_fn(NativeFocus<F,L,R>)->O)
    ensures observe(decode(encode(s)))==observe(s)
{}
pub struct Memory<F,L,R> {pub viewport_id:Id,pub focus:Map<Id,NativeFocus<F,L,R>>}
pub open spec fn native_memory_focused<F,L,R>(s:Memory<F,L,R>)->Option<Id> {
    if s.focus.dom().contains(s.viewport_id) {native_focused(s.focus[s.viewport_id])} else {None}
}
pub open spec fn encode_viewports<F,L,R>(s:Memory<F,L,R>)->Map<Id,Navigation<F,L,R>> {
    Map::new(s.focus.dom(),|id:Id| encode(s.focus[id]))
}
pub proof fn every_viewport_and_pending_owner_is_preserved<F,L,R>(s:Memory<F,L,R>)
    ensures encode_viewports(s).dom()==s.focus.dom(),
        forall |id:Id| s.focus.dom().contains(id) ==>
            decode(#[trigger] encode_viewports(s)[id])==s.focus[id],
        native_memory_focused(s)==if encode_viewports(s).dom().contains(s.viewport_id){
            specified_focused(encode_viewports(s)[s.viewport_id])
        }else{None}
{}
}
