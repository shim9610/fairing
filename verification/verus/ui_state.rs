//! Joint UI specification. Product refinement is a separate obligation.
use vstd::prelude::*;

verus! {
pub type Id = u64;

pub struct Shell {
    pub faulted: bool, pub prompt: bool, pub lock: bool, pub info: bool,
    pub overlay: bool, pub keyboard: bool, pub overview: bool, pub home: bool,
    pub modal_scope: Option<Id>,
    pub recovery_enabled:bool,
}
pub enum Role { Content, Authentication, Recovery }
pub struct Capabilities {pub activate:bool,pub focus:bool,pub edit:bool,pub popup:bool,pub drag:bool}
pub struct Widget {
    pub mounted: bool, pub enabled: bool, pub scope: Option<Id>,
    pub role:Role, pub capabilities:Capabilities,
    pub intended_hit:spec_fn(PointBits)->bool,
}
pub struct RectBits { pub min_x:u32, pub min_y:u32, pub max_x:u32, pub max_y:u32 }
pub struct PointBits { pub x:u32, pub y:u32 }
pub struct Surface {
    pub rect:RectBits, pub clip:RectBits, pub layer:int, pub painted:bool,
    // Actual routing after clipping, z-order, gestures and input interception.
    pub hit:spec_fn(PointBits)->bool,
}
pub enum Phase { Hidden, Entering, Shown, Leaving }
pub struct Animation { pub phase:Phase, pub progress_bits:u32, pub epoch:u64 }
pub struct Lease<T> { pub seen:u64, pub press_scoped:bool, pub payload:T }

pub struct UiState<V, R> {
    pub shell:Shell,
    pub widgets:Map<Id,Widget>, pub values:Map<Id,V>,
    pub focus:Option<Id>, pub keyboard_owner:Option<Id>,
    pub capture:Map<u64,Id>,
    // Intent and retained rendering memory are deliberately distinct.
    pub popup_intent:Set<Id>, pub popup_order:Seq<Id>,
    pub animations:Map<Id,Animation>, pub surfaces:Map<Id,Surface>,
    pub pass:u64, pub leases:Map<Id,Lease<R>>,
}

pub enum ShellGroup { Faulted, Locked, Unlock, Info, Overlay, Keyboard, Overview, Home, Workspace }
pub enum WidgetGroup { Absent, Disabled, Faulted, Locked, OutsideModal, Ready }
pub enum FocusGroup { None, This, Other, Dangling }
pub enum PopupGroup { Closed, MissingSurface, StaleSurface, Open }
pub enum CaptureGroup { Free, This, Other, Dangling }
pub enum MotionGroup { Missing, Hidden, Entering, Shown, Leaving }
pub enum SurfaceGroup { Missing, Hidden, Blocked, Reachable }
pub struct Groups {
    pub shell:ShellGroup, pub widget:WidgetGroup, pub focus:FocusGroup,
    pub popup:PopupGroup, pub capture:CaptureGroup, pub motion:MotionGroup,
    pub surface:SurfaceGroup,
    pub capabilities:Option<Capabilities>,
}

pub open spec fn shell_group(s:Shell)->ShellGroup {
    if s.faulted {ShellGroup::Faulted} else if s.prompt {
        if s.lock {ShellGroup::Locked} else {ShellGroup::Unlock}
    } else if s.info {ShellGroup::Info} else if s.overlay {ShellGroup::Overlay}
    else if s.keyboard {ShellGroup::Keyboard} else if s.overview {ShellGroup::Overview}
    else if s.home {ShellGroup::Home} else {ShellGroup::Workspace}
}
pub open spec fn shell_member(s:Shell,g:ShellGroup)->bool {
    match g {
        ShellGroup::Faulted=>s.faulted,
        ShellGroup::Locked=>!s.faulted && s.prompt && s.lock,
        ShellGroup::Unlock=>!s.faulted && s.prompt && !s.lock,
        ShellGroup::Info=>!s.faulted && !s.prompt && s.info,
        ShellGroup::Overlay=>!s.faulted && !s.prompt && !s.info && s.overlay,
        ShellGroup::Keyboard=>!s.faulted && !s.prompt && !s.info && !s.overlay && s.keyboard,
        ShellGroup::Overview=>!s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && s.overview,
        ShellGroup::Home=>!s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && !s.overview && s.home,
        ShellGroup::Workspace=>!s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && !s.overview && !s.home,
    }
}
pub open spec fn mounted<V,R>(s:UiState<V,R>,id:Id)->bool {
    s.widgets.dom().contains(id) && s.widgets[id].mounted
}
pub open spec fn enabled<V,R>(s:UiState<V,R>,id:Id)->bool {
    mounted(s,id) && s.widgets[id].enabled
}
pub open spec fn in_scope<V,R>(s:UiState<V,R>,id:Id)->bool {
    recovery_widget(s,id) || s.shell.modal_scope.is_none() || s.widgets[id].scope == s.shell.modal_scope
}
pub open spec fn recovery_widget<V,R>(s:UiState<V,R>,id:Id)->bool {
    s.shell.faulted && s.shell.recovery_enabled && mounted(s,id) && s.widgets[id].role==Role::Recovery
}
pub open spec fn quarantined<V,R>(s:UiState<V,R>,id:Id)->bool {
    s.shell.faulted && !recovery_widget(s,id)
}
pub open spec fn locked<V,R>(s:UiState<V,R>,id:Id)->bool {
    !s.shell.faulted && s.shell.prompt && s.shell.lock && s.widgets[id].role!=Role::Authentication
}
pub open spec fn ready<V,R>(s:UiState<V,R>,id:Id)->bool {
    enabled(s,id) && !quarantined(s,id) && !locked(s,id) && in_scope(s,id)
}
pub open spec fn widget_group<V,R>(s:UiState<V,R>,id:Id)->WidgetGroup {
    if !mounted(s,id) {WidgetGroup::Absent} else if !s.widgets[id].enabled {WidgetGroup::Disabled}
    else if quarantined(s,id) {WidgetGroup::Faulted} else if locked(s,id) {WidgetGroup::Locked}
    else if !in_scope(s,id) {WidgetGroup::OutsideModal} else {WidgetGroup::Ready}
}
pub open spec fn widget_member<V,R>(s:UiState<V,R>,id:Id,g:WidgetGroup)->bool {
    match g {
        WidgetGroup::Absent=>!mounted(s,id),
        WidgetGroup::Disabled=>mounted(s,id) && !s.widgets[id].enabled,
        WidgetGroup::Faulted=>enabled(s,id) && quarantined(s,id),
        WidgetGroup::Locked=>enabled(s,id) && !quarantined(s,id) && locked(s,id),
        WidgetGroup::OutsideModal=>enabled(s,id) && !quarantined(s,id) && !locked(s,id) && !in_scope(s,id),
        WidgetGroup::Ready=>ready(s,id),
    }
}
pub open spec fn live_owner<V,R>(s:UiState<V,R>,owner:Option<Id>)->bool {
    match owner { None=>false, Some(id)=>mounted(s,id) }
}
pub open spec fn focus_group<V,R>(s:UiState<V,R>,id:Id)->FocusGroup {
    match s.focus { None=>FocusGroup::None, Some(owner)=>
        if !mounted(s,owner) {FocusGroup::Dangling} else if owner==id {FocusGroup::This} else {FocusGroup::Other} }
}
pub open spec fn focus_member<V,R>(s:UiState<V,R>,id:Id,g:FocusGroup)->bool {
    match g {
        FocusGroup::None=>s.focus.is_none(),
        FocusGroup::Dangling=>s.focus.is_some() && !live_owner(s,s.focus),
        FocusGroup::This=>s.focus==Some(id) && mounted(s,id),
        FocusGroup::Other=>live_owner(s,s.focus) && s.focus!=Some(id),
    }
}
pub open spec fn popup_group<V,R>(s:UiState<V,R>,id:Id)->PopupGroup {
    if s.popup_intent.contains(id) {
        if s.popup_order.contains(id) {PopupGroup::Open} else {PopupGroup::MissingSurface}
    } else if s.popup_order.contains(id) {PopupGroup::StaleSurface} else {PopupGroup::Closed}
}
pub open spec fn popup_member<V,R>(s:UiState<V,R>,id:Id,g:PopupGroup)->bool {
    match g {
        PopupGroup::Closed=>!s.popup_intent.contains(id) && !s.popup_order.contains(id),
        PopupGroup::MissingSurface=>s.popup_intent.contains(id) && !s.popup_order.contains(id),
        PopupGroup::StaleSurface=>!s.popup_intent.contains(id) && s.popup_order.contains(id),
        PopupGroup::Open=>s.popup_intent.contains(id) && s.popup_order.contains(id),
    }
}
pub open spec fn captured<V,R>(s:UiState<V,R>,pointer:u64)->Option<Id> {
    if s.capture.dom().contains(pointer) {Some(s.capture[pointer])} else {None}
}
pub open spec fn capture_group<V,R>(s:UiState<V,R>,id:Id,pointer:u64)->CaptureGroup {
    match captured(s,pointer) { None=>CaptureGroup::Free, Some(owner)=>
        if !mounted(s,owner) {CaptureGroup::Dangling} else if owner==id {CaptureGroup::This} else {CaptureGroup::Other} }
}
pub open spec fn capture_member<V,R>(s:UiState<V,R>,id:Id,pointer:u64,g:CaptureGroup)->bool {
    match g {
        CaptureGroup::Free=>captured(s,pointer).is_none(),
        CaptureGroup::Dangling=>captured(s,pointer).is_some() && !live_owner(s,captured(s,pointer)),
        CaptureGroup::This=>captured(s,pointer)==Some(id) && mounted(s,id),
        CaptureGroup::Other=>live_owner(s,captured(s,pointer)) && captured(s,pointer)!=Some(id),
    }
}
pub open spec fn motion_group<V,R>(s:UiState<V,R>,id:Id)->MotionGroup {
    if !s.animations.dom().contains(id) {MotionGroup::Missing} else {match s.animations[id].phase {
        Phase::Hidden=>MotionGroup::Hidden, Phase::Entering=>MotionGroup::Entering,
        Phase::Shown=>MotionGroup::Shown, Phase::Leaving=>MotionGroup::Leaving,
    }}
}
pub open spec fn motion_member<V,R>(s:UiState<V,R>,id:Id,g:MotionGroup)->bool {
    match g {
        MotionGroup::Missing=>!s.animations.dom().contains(id),
        MotionGroup::Hidden=>s.animations.dom().contains(id) && s.animations[id].phase==Phase::Hidden,
        MotionGroup::Entering=>s.animations.dom().contains(id) && s.animations[id].phase==Phase::Entering,
        MotionGroup::Shown=>s.animations.dom().contains(id) && s.animations[id].phase==Phase::Shown,
        MotionGroup::Leaving=>s.animations.dom().contains(id) && s.animations[id].phase==Phase::Leaving,
    }
}
pub open spec fn has_hit<V,R>(s:UiState<V,R>,id:Id)->bool {
    s.surfaces.dom().contains(id) && exists |p:PointBits| #[trigger] (s.surfaces[id].hit)(p)
}
pub open spec fn surface_group<V,R>(s:UiState<V,R>,id:Id)->SurfaceGroup {
    if !s.surfaces.dom().contains(id) {SurfaceGroup::Missing}
    else if !s.surfaces[id].painted {SurfaceGroup::Hidden}
    else if !has_hit(s,id) {SurfaceGroup::Blocked} else {SurfaceGroup::Reachable}
}
pub open spec fn surface_member<V,R>(s:UiState<V,R>,id:Id,g:SurfaceGroup)->bool {
    match g {
        SurfaceGroup::Missing=>!s.surfaces.dom().contains(id),
        SurfaceGroup::Hidden=>s.surfaces.dom().contains(id) && !s.surfaces[id].painted,
        SurfaceGroup::Blocked=>s.surfaces.dom().contains(id) && s.surfaces[id].painted && !has_hit(s,id),
        SurfaceGroup::Reachable=>s.surfaces.dom().contains(id) && s.surfaces[id].painted && has_hit(s,id),
    }
}
pub open spec fn classify<V,R>(s:UiState<V,R>,id:Id,pointer:u64)->Groups {
    Groups {shell:shell_group(s.shell),widget:widget_group(s,id),focus:focus_group(s,id),
        popup:popup_group(s,id),capture:capture_group(s,id,pointer),motion:motion_group(s,id),surface:surface_group(s,id),
        capabilities:capabilities(s,id)}
}
pub open spec fn capabilities<V,R>(s:UiState<V,R>,id:Id)->Option<Capabilities> {
    if s.widgets.dom().contains(id) {Some(s.widgets[id].capabilities)} else {None}
}
pub open spec fn member<V,R>(s:UiState<V,R>,id:Id,pointer:u64,g:Groups)->bool {
    shell_member(s.shell,g.shell) && widget_member(s,id,g.widget) && focus_member(s,id,g.focus)
    && popup_member(s,id,g.popup) && capture_member(s,id,pointer,g.capture)
    && motion_member(s,id,g.motion) && surface_member(s,id,g.surface) && g.capabilities==capabilities(s,id)
}
// All modeled states, including dangling owners, conflicting shell predicates,
// missing surfaces and stale popups. No well-formedness or reachability assumption.
pub proof fn joint_partition<V,R>(s:UiState<V,R>,id:Id,pointer:u64)
    ensures member(s,id,pointer,classify(s,id,pointer)),
        forall |g:Groups| member(s,id,pointer,g) ==> g==classify(s,id,pointer)
{}
pub proof fn unbounded_collection_partition<V,R>(s:UiState<V,R>)
    ensures forall |id:Id,p:u64| member(s,id,p,#[trigger] classify(s,id,p))
{
    assert forall |id:Id,p:u64| member(s,id,p,#[trigger] classify(s,id,p)) by {joint_partition(s,id,p);}
}

pub enum Operation { Activate, Focus, Text, OpenPopup, DismissPopup, BeginCapture, EndCapture, Cancel }
pub enum Invocation { Pointer, Keyboard, Programmatic }
pub struct Action {pub target:Id, pub pointer:u64, pub operation:Operation,pub invocation:Invocation,pub fresh_press:bool}
pub enum Effect { Activated(Id), Focused(Id), Edited(Id), Opened(Id), Dismissed(Id), Captured(Id,u64), Released(Id,u64), Cancelled(Id) }
pub open spec fn promised<V,R>(s:UiState<V,R>,a:Action)->bool {
    match a.operation {
        Operation::DismissPopup | Operation::EndCapture | Operation::Cancel=>true,
        Operation::Activate=>ready(s,a.target) && s.widgets[a.target].capabilities.activate,
        Operation::Focus=>ready(s,a.target) && s.widgets[a.target].capabilities.focus,
        Operation::Text=>ready(s,a.target) && s.widgets[a.target].capabilities.edit && s.focus==Some(a.target) && a.invocation==Invocation::Keyboard,
        Operation::OpenPopup=>ready(s,a.target) && s.widgets[a.target].capabilities.popup,
        Operation::BeginCapture=>ready(s,a.target) && s.widgets[a.target].capabilities.drag
            && a.invocation==Invocation::Pointer && a.fresh_press,
    }
}
// Frames must realize the independently promised operation. A missing hit region
// is a failed realization obligation, not a reason to withdraw the promise.
pub open spec fn needs_pointer(a:Action)->bool {
    a.invocation==Invocation::Pointer && match a.operation {
        Operation::Activate | Operation::Focus | Operation::OpenPopup | Operation::BeginCapture=>true,
        Operation::Text | Operation::DismissPopup | Operation::EndCapture | Operation::Cancel=>false,
    }
}
pub open spec fn input_realizable<V,R>(s:UiState<V,R>,a:Action)->bool {
    !needs_pointer(a) || (surface_member(s,a.target,SurfaceGroup::Reachable)
        && (exists |p:PointBits| #[trigger] (s.widgets[a.target].intended_hit)(p))
        && (forall |p:PointBits| #[trigger] (s.widgets[a.target].intended_hit)(p) ==> (s.surfaces[a.target].hit)(p)))
}
pub open spec fn same_contract<V,R>(a:UiState<V,R>,b:UiState<V,R>)->bool {
    a.shell==b.shell && a.widgets==b.widgets && a.focus==b.focus
}
pub proof fn stale_presentation_cannot_withdraw_promises<V,R>(a:UiState<V,R>,b:UiState<V,R>,op:Action)
    requires same_contract(a,b)
    ensures promised(a,op)==promised(b,op)
{}
pub proof fn text_has_one_recipient<V,R>(s:UiState<V,R>,a:Action,b:Action)
    requires a.operation==Operation::Text,b.operation==Operation::Text,promised(s,a),promised(s,b)
    ensures a.target==b.target
{}
pub proof fn group_determines_promises<V,R>(a:UiState<V,R>,b:UiState<V,R>,op:Action)
    requires classify(a,op.target,op.pointer)==classify(b,op.target,op.pointer)
    ensures promised(a,op)==promised(b,op)
{}
pub proof fn focus_identity_is_observable<V,R>(a:UiState<V,R>,b:UiState<V,R>,op:Action)
    requires op.operation==Operation::Text,ready(a,op.target),ready(b,op.target),
        a.widgets[op.target].capabilities.edit,b.widgets[op.target].capabilities.edit,
        op.invocation==Invocation::Keyboard,
        a.focus==Some(op.target),b.focus!=Some(op.target)
    ensures promised(a,op),!promised(b,op)
{}
pub proof fn blocked_promised_input_is_a_violation<V,R>(s:UiState<V,R>,a:Action)
    requires promised(s,a),needs_pointer(a),!has_hit(s,a.target)
    ensures !input_realizable(s,a)
{}
pub proof fn fault_quarantine_and_cleanup<V,R>(s:UiState<V,R>,a:Action)
    requires s.shell.faulted,!recovery_widget(s,a.target)
    ensures promised(s,a) == (a.operation==Operation::DismissPopup || a.operation==Operation::EndCapture || a.operation==Operation::Cancel)
{}
pub proof fn recovery_contract_survives_fault<V,R>(s:UiState<V,R>,id:Id)
    requires enabled(s,id),s.shell.faulted,s.shell.recovery_enabled,s.widgets[id].role==Role::Recovery
    ensures ready(s,id)
{}
pub proof fn authentication_contract_survives_lock<V,R>(s:UiState<V,R>,id:Id)
    requires enabled(s,id),!s.shell.faulted,s.shell.prompt,s.shell.lock,
        s.widgets[id].role==Role::Authentication,in_scope(s,id)
    ensures ready(s,id)
{}
pub open spec fn without_owner(order:Seq<Id>,id:Id)->Seq<Id> {
    order.filter(|owner:Id| owner!=id)
}
pub open spec fn outcome<V,R>(before:UiState<V,R>,a:Action,after:UiState<V,R>,e:Effect)->bool {
    promised(before,a) && match a.operation {
        Operation::Activate=>e==Effect::Activated(a.target),
        Operation::Focus=>e==Effect::Focused(a.target) && after.focus==Some(a.target),
        Operation::Text=>e==Effect::Edited(a.target) && before.focus==Some(a.target),
        Operation::OpenPopup=>e==Effect::Opened(a.target) && after.popup_intent==before.popup_intent.insert(a.target)
            && after.popup_order==without_owner(before.popup_order,a.target).push(a.target),
        Operation::DismissPopup=>e==Effect::Dismissed(a.target) && after.popup_intent==before.popup_intent.remove(a.target)
            && after.popup_order==without_owner(before.popup_order,a.target),
        Operation::BeginCapture=>e==Effect::Captured(a.target,a.pointer) && after.capture==before.capture.insert(a.pointer,a.target),
        Operation::EndCapture=>e==Effect::Released(a.target,a.pointer)
            && after.capture==if captured(before,a.pointer)==Some(a.target) {before.capture.remove(a.pointer)} else {before.capture},
        Operation::Cancel=>e==Effect::Cancelled(a.target)
            && after.focus==if before.focus==Some(a.target) {None} else {before.focus}
            && after.keyboard_owner==if before.keyboard_owner==Some(a.target) {None} else {before.keyboard_owner}
            && after.popup_intent==before.popup_intent.remove(a.target)
            && after.popup_order==without_owner(before.popup_order,a.target)
            && (forall |p:u64| #[trigger] captured(after,p)==if captured(before,p)==Some(a.target) {None} else {captured(before,p)}),
    }
}
pub open spec fn cancelled<V,R>(s:UiState<V,R>,id:Id)->UiState<V,R> {
    UiState {
        focus:if s.focus==Some(id) {None} else {s.focus},
        keyboard_owner:if s.keyboard_owner==Some(id) {None} else {s.keyboard_owner},
        capture:s.capture.restrict(s.capture.dom().filter(|p:u64| s.capture[p]!=id)),
        popup_intent:s.popup_intent.remove(id),popup_order:without_owner(s.popup_order,id),
        ..s
    }
}
// Specification consistency: cancellation has a successor from every modeled
// state, including arbitrary stale owners. This is not a concrete execution proof.
pub proof fn cancellation_has_a_successor<V,R>(s:UiState<V,R>,a:Action)
    requires a.operation==Operation::Cancel
    ensures outcome(s,a,cancelled(s,a.target),Effect::Cancelled(a.target))
{
    let next=cancelled(s,a.target);
    assert forall |p:u64| #[trigger] captured(next,p)==if captured(s,p)==Some(a.target) {None} else {captured(s,p)} by {
        if s.capture.dom().contains(p) {
            assert(s.capture.dom().filter(|owner:u64| s.capture[owner]!=a.target).contains(p)
                == (s.capture[p]!=a.target));
        }
    }
}
pub proof fn release_preserves_a_new_owner<V,R>(s:UiState<V,R>,a:Action,next:UiState<V,R>)
    requires a.operation==Operation::EndCapture,captured(s,a.pointer)!=Some(a.target),
        outcome(s,a,next,Effect::Released(a.target,a.pointer))
    ensures next.capture==s.capture
{}
// Total concrete projection and observation preservation are required separately
// from partitioning values of UiState. None is an explicit unmapped concrete state.
pub open spec fn abstraction_obligation<C,O,V,R>(
    reachable:spec_fn(C)->bool,project:spec_fn(C)->Option<UiState<V,R>>,
    concrete_observation:spec_fn(C)->O,model_observation:spec_fn(UiState<V,R>)->O)->bool {
    forall |c:C| #[trigger] reachable(c) ==> match #[trigger] project(c) {
        None=>false, Some(model)=>concrete_observation(c)==model_observation(model)
    }
}
// Requires a concrete transition relation and invocation contract to discharge.
// No assume/admit/external-body axiom supplies those missing product bindings.
pub open spec fn product_progress_obligation<V,R>(s:UiState<V,R>,a:Action,bound:nat,
    runs:spec_fn(UiState<V,R>,Action,spec_fn(nat)->UiState<V,R>,spec_fn(nat)->Option<Effect>)->bool)->bool {
    promised(s,a) ==> input_realizable(s,a)
        && (exists |trace:spec_fn(nat)->UiState<V,R>,outputs:spec_fn(nat)->Option<Effect>|
            #[trigger] runs(s,a,trace,outputs))
        && (forall |trace:spec_fn(nat)->UiState<V,R>,outputs:spec_fn(nat)->Option<Effect>|
            #[trigger] runs(s,a,trace,outputs) ==> trace(0)==s
                && (exists |n:nat| n<=bound && match #[trigger] outputs(n) {
                    Some(effect)=>outcome(s,a,trace(n),effect),None=>false
                })
                && (forall |n:nat| match #[trigger] outputs(n) {
                    Some(effect)=>outcome(s,a,trace(n),effect),None=>true
                })
                && (forall |m:nat,n:nat| #[trigger] outputs(m).is_some() && #[trigger] outputs(n).is_some() ==> m==n))
}
}
