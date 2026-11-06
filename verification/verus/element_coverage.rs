//! Unbounded entry-cut induction for a compiler graph's checked closure regions.
use vstd::prelude::*;
verus! {

pub open spec fn cut(edges: Set<(int, int)>, region: Set<int>, guards: Set<int>) -> bool {
    forall |a:int, b:int| #[trigger] edges.contains((a,b))
        && !region.contains(a) && region.contains(b) ==> guards.contains(a)
}

pub proof fn outside_at(
    path: Seq<int>, edges: Set<(int,int)>, region: Set<int>, guards: Set<int>, n: int,
)
    requires
        0 <= n < path.len(),
        !region.contains(path[0]),
        cut(edges, region, guards),
        forall |i:int| #![trigger path[i]] 0 <= i < path.len()-1 ==> edges.contains((path[i], path[i+1])),
        forall |i:int| 0 <= i < path.len() ==> !guards.contains(#[trigger] path[i]),
    ensures !region.contains(path[n]),
    decreases n,
{
    if n > 0 {
        outside_at(path, edges, region, guards, n-1);
        assert(0 <= n-1 < path.len()-1);
        assert(edges.contains((path[n-1], path[(n-1)+1])));
        assert(!guards.contains(path[n-1]));
    }
}

// Any finite execution length, with no fixed path-length or recursion cutoff.
// This theorem applies to the extracted graph when the concrete cut and root
// conditions have been checked. It does not establish omitted dispatch edges.
pub proof fn unguarded_paths_cannot_enter_callbacks(
    path: Seq<int>, edges: Set<(int,int)>, region: Set<int>, guards: Set<int>,
)
    requires
        path.len() > 0,
        !region.contains(path[0]),
        cut(edges, region, guards),
        forall |i:int| #![trigger path[i]] 0 <= i < path.len()-1 ==> edges.contains((path[i], path[i+1])),
        forall |i:int| 0 <= i < path.len() ==> !guards.contains(#[trigger] path[i]),
    ensures forall |i:int| 0 <= i < path.len() ==> !region.contains(#[trigger] path[i]),
{
    assert forall |i:int| 0 <= i < path.len() implies !region.contains(path[i]) by {
        outside_at(path, edges, region, guards, i);
    }
}
}
