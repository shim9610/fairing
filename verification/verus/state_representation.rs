//! Lossless state representation only; no operation or progress contract.
use vstd::prelude::*;

verus! {
pub struct ConcreteState<R,World> { pub roots:R, pub world:World }
pub struct SpecifiedState<S,World> { pub roots:S, pub world:World }

// TypeKey is exact nominal identity, including generic and const instantiations.
// A source declaration name or a hash is not substituted for PhantomData<Self>.
pub struct Nominal<TypeKey,V> { pub native_type:TypeKey, pub payload:V }
pub open spec fn encode_nominal<K,R,S>(value:Nominal<K,R>,encode:spec_fn(R)->S)->Nominal<K,S> {
    Nominal { native_type:value.native_type, payload:encode(value.payload) }
}
pub open spec fn decode_nominal<K,R,S>(value:Nominal<K,S>,decode:spec_fn(S)->Option<R>)->Option<Nominal<K,R>> {
    match decode(value.payload) {
        Some(payload)=>Some(Nominal { native_type:value.native_type, payload }),
        None=>None,
    }
}
pub proof fn nominal_identity_is_preserved<K,R,S>(value:Nominal<K,R>,
    encode:spec_fn(R)->S,decode:spec_fn(S)->Option<R>)
    requires forall |r:R| #[trigger] decode(encode(r))==Some(r)
    ensures decode_nominal(encode_nominal(value,encode),decode)==Some(value)
{}

pub open spec fn represent<R,S,World>(state:ConcreteState<R,World>,
    encode:spec_fn(R)->S)->SpecifiedState<S,World> {
    SpecifiedState { roots:encode(state.roots), world:state.world }
}
pub open spec fn reconstruct<R,S,World>(state:SpecifiedState<S,World>,
    decode:spec_fn(S)->Option<R>)->Option<ConcreteState<R,World>> {
    match decode(state.roots) {
        Some(roots)=>Some(ConcreteState { roots, world:state.world }),
        None=>None,
    }
}
pub proof fn whole_state_roundtrip<R,S,World>(state:ConcreteState<R,World>,
    encode:spec_fn(R)->S,decode:spec_fn(S)->Option<R>)
    requires forall |r:R| #[trigger] decode(encode(r))==Some(r)
    ensures reconstruct(represent(state,encode),decode)==Some(state)
{}
pub proof fn all_observations_are_preserved<R,S,World,O>(state:ConcreteState<R,World>,
    encode:spec_fn(R)->S,decode:spec_fn(S)->Option<R>,observe:spec_fn(ConcreteState<R,World>)->O)
    requires forall |r:R| #[trigger] decode(encode(r))==Some(r)
    ensures match reconstruct(represent(state,encode),decode) {
        Some(restored)=>observe(restored)==observe(state),None=>false
    }
{}
pub proof fn different_states_cannot_collapse<R,S,World>(a:ConcreteState<R,World>,b:ConcreteState<R,World>,
    encode:spec_fn(R)->S,decode:spec_fn(S)->Option<R>)
    requires forall |r:R| #[trigger] decode(encode(r))==Some(r),
        represent(a,encode)==represent(b,encode)
    ensures a==b
{ whole_state_roundtrip(a,encode,decode); whole_state_roundtrip(b,encode,decode); }
pub proof fn exactly_one_representation<R,S,World>(state:ConcreteState<R,World>,
    first:SpecifiedState<S,World>,second:SpecifiedState<S,World>,
    encode:spec_fn(R)->S,decode:spec_fn(S)->Option<R>)
    requires forall |model:S| match #[trigger] decode(model) {
        Some(value)=>encode(value)==model,None=>false
    }, reconstruct(first,decode)==Some(state),reconstruct(second,decode)==Some(state)
    ensures first==second
{}

pub open spec fn encode_sequence<R,S>(roots:Seq<R>,encode:spec_fn(R)->S)->Seq<S> {
    Seq::new(roots.len(),|i:int| encode(roots[i]))
}
pub open spec fn decode_sequence<R,S>(roots:Seq<S>,decode:spec_fn(S)->Option<R>)->Seq<Option<R>> {
    Seq::new(roots.len(),|i:int| decode(roots[i]))
}
pub proof fn arbitrary_root_count<R,S>(roots:Seq<R>,encode:spec_fn(R)->S,decode:spec_fn(S)->Option<R>)
    requires forall |r:R| #[trigger] decode(encode(r))==Some(r)
    ensures decode_sequence(encode_sequence(roots,encode),decode)
        =~= Seq::new(roots.len(),|i:int| Some(roots[i]))
{
    assert forall |i:int| 0<=i<roots.len() implies
        #[trigger] decode(encode(roots[i]))==Some(roots[i]) by {}
}
pub enum Sum<A,B> { Left(A), Right(B) }
pub open spec fn encode_sum<A,B,S,T>(value:Sum<A,B>,
    left:spec_fn(A)->S,right:spec_fn(B)->T)->Sum<S,T> {
    match value { Sum::Left(a)=>Sum::Left(left(a)),Sum::Right(b)=>Sum::Right(right(b)) }
}
pub open spec fn decode_sum<A,B,S,T>(value:Sum<S,T>,
    left:spec_fn(S)->Option<A>,right:spec_fn(T)->Option<B>)->Option<Sum<A,B>> {
    match value {
        Sum::Left(s)=>match left(s) {Some(a)=>Some(Sum::Left(a)),None=>None},
        Sum::Right(t)=>match right(t) {Some(b)=>Some(Sum::Right(b)),None=>None},
    }
}
pub proof fn heterogeneous_roots<A,B,S,T>(value:Sum<A,B>,
    encode_a:spec_fn(A)->S,decode_a:spec_fn(S)->Option<A>,
    encode_b:spec_fn(B)->T,decode_b:spec_fn(T)->Option<B>)
    requires forall |a:A| #[trigger] decode_a(encode_a(a))==Some(a),
        forall |b:B| #[trigger] decode_b(encode_b(b))==Some(b)
    ensures decode_sum(encode_sum(value,encode_a,encode_b),decode_a,decode_b)==Some(value)
{}
pub open spec fn encode_store<K,R,S>(roots:Map<K,R>,encode:spec_fn(R)->S)->Map<K,S> {
    Map::new(roots.dom(),|key:K| encode(roots[key]))
}
pub proof fn identities_aliases_and_all_store_entries<K,R,S>(roots:Map<K,R>,
    encode:spec_fn(R)->S,decode:spec_fn(S)->Option<R>)
    requires forall |r:R| #[trigger] decode(encode(r))==Some(r)
    ensures encode_store(roots,encode).dom()==roots.dom(),
        forall |key:K| roots.dom().contains(key) ==>
            #[trigger] decode(encode_store(roots,encode)[key])==Some(roots[key])
{}
}
