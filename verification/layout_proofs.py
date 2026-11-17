#!/usr/bin/env python3
"""Prove production layout-token lifetime and issuer-observation handling.

Production method bodies are unchanged. Instant is represented by symbolic
nanosecond ticks: the layout contract uses only equality; snapshot elapsed time
covers the complete u64 observation domain. Shared observation schedules model
atomic reads, not their concurrent implementation (verified separately).
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

import install
from validation import LAYOUT_INPUTS, require
sys.path.insert(0, str(Path(__file__).resolve().parent / 'verus'))
from mediation_path import function

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/assurance/layout-token'
TOKEN = 'crates/fairing/src/elements/layout_token.rs'
ACCESS = 'crates/fairing/src/access/mod.rs'
CX = 'crates/fairing/src/screen/cx.rs'
SESSION = 'crates/fairing/src/access/session.rs'

SUPPORT = r'''#![allow(dead_code)]
use std::cell::Cell;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Instant(u128);
impl Instant {
    fn saturating_duration_since(self, origin: Self) -> Elapsed {
        Elapsed(self.0.saturating_sub(origin.0))
    }
}
struct Elapsed(u128);
impl Elapsed { fn as_nanos(self)->u128 {self.0} }
#[path = "@RULES@"] mod session_rules;
#[derive(Clone, Copy)]
struct Read { revoked: bool, before: u64, after: u64, until: u64 }
impl Read {
    fn any() -> Self { Self { revoked:kani::any(), before:kani::any(), after:kani::any(), until:kani::any() } }
    fn stamp(self, now: Instant, id: access::SessionId) -> Option<(access::SessionId,u64)> {
        // Independent expectation: do not call production observe here.
        let elapsed=if now.0 > u128::from(u64::MAX) {u64::MAX} else {now.0 as u64};
        (!self.revoked && elapsed < self.until && self.before == self.after && self.before % 2 == 0)
            .then_some((id,self.before))
    }
}
struct Shared { first: Read, second: Read, read: Cell<bool> }
impl Shared {
    fn observe(&self, now:u64) -> session_rules::Observation {
        let state=if self.read.replace(true) {self.second} else {self.first};
        session_rules::observe(state.revoked,state.before,state.after,now,state.until)
    }
}
mod access {
    use super::*;
    use std::convert::TryFrom;
    use crate::screen::Cx;
    use session_rules::Observation;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct SessionId(pub u64);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Gate(pub u64);
    pub trait Permission: Copy { fn gate(self) -> Gate; }
    impl Permission for u64 { fn gate(self)->Gate {Gate(self)} }
    pub struct PermissionBinding<P:Permission> { required:P, gate:Gate }
    impl<P:Permission> PermissionBinding<P> {
        @BIND_NEW@
        @BIND_GATE@
        @BIND_ALLOWS@
    }
    struct Inner { id:SessionId, origin:Instant, shared:Shared }
    struct SessionToken(Inner);
    #[derive(Debug)] enum SessionReadError { Revoked, Expired, Updating }
    struct SessionSnapshot<'a> { token:&'a SessionToken, revision:u64, now:u64 }
    impl SessionSnapshot<'_> { @REVISION@ }
    impl SessionToken { @ID@ @SNAPSHOT@ }
    pub struct Access {
        pub quarantined:bool, pub token_sessions:bool, pub token_policy:bool,
        token:Option<SessionToken>, token_now:Instant, pub grant:bool,
    }
    impl Access {
        @AUTHORITY@
        @CACHEABLE@
        @TOKEN_MODE@
        pub fn allows(&self, _: &Gate)->bool {self.grant}
        pub fn new(now:Instant, first:Read, second:Read, id:SessionId, present:bool,
                   quarantined:bool, mode:bool, policy:bool, grant:bool)->Self {
            Self {quarantined,token_sessions:mode,token_policy:policy,token_now:now,grant,
                token:present.then_some(SessionToken(Inner {id,origin:Instant(0),
                    shared:Shared {first,second,read:Cell::new(false)}}))}
        }
    }
}
pub use access::Gate;
mod screen {
    use super::*;
    use access::{Access, Gate};
    pub struct Cx<'a> {pub now:Instant,pub frame:u64,pub access:&'a Access}
    impl Cx<'_> {
        @CX_FRAME@
        @CX_CACHEABLE@
        @CX_AUTHORITY@
        @CX_MODE@
        @CX_ALLOWS@
    }
}
mod elements { mod layout_token {
@TOKEN@
@CLIENT@
} }
'''

CLIENT = r'''
#[kani::proof]
fn layout_mint_binds_the_checked_observation_and_permission() {
    let now=crate::Instant(kani::any());let frame:u64=kani::any();let id=SessionId(kani::any());
    let first=Read::any();let second=Read::any();let present:bool=kani::any();
    let quarantined:bool=kani::any();let mode:bool=kani::any();let policy:bool=kani::any();let grant:bool=kani::any();
    let access=access::Access::new(now,first,second,id,present,quarantined,mode,policy,grant);
    let cx=Cx{now,frame,access:&access};let stamp=(SessionId(kani::any()),kani::any());let key:u64=kani::any();
    let live=present && !quarantined && mode;
    let expected=live && first.stamp(now,id)==Some(stamp) && grant && second.stamp(now,id)==Some(stamp);
    let token=LayoutToken::new(PermissionBinding::new(key),stamp,&cx);
    assert_eq!(token.is_some(),expected,"layout mint admitted an invalid observation");
    if let Some(token)=token {
        assert!(token.frame==frame && token.now==now && token.stamp==stamp
            && token.gate()==&access::Gate(key) && token.cacheable==(mode && !policy),
            "layout mint changed its data binding");
    }
}
#[kani::proof]
fn layout_reuse_requires_same_frame_time_and_live_identity_revision() {
    let now=crate::Instant(kani::any());let frame:u64=kani::any();let id=SessionId(kani::any());
    let first=Read::any();let second=Read::any();let present:bool=kani::any();
    let quarantined:bool=kani::any();let mode:bool=kani::any();let grant:bool=kani::any();let cacheable:bool=kani::any();
    let access=access::Access::new(now,first,second,id,present,quarantined,mode,!cacheable,grant);
    let cx=Cx{now,frame,access:&access};
    let token=LayoutToken {binding:PermissionBinding::new(0u64),stamp:(SessionId(kani::any()),kani::any()),
        now:crate::Instant(kani::any()),frame:kani::any(),cacheable};
    let live=present && !quarantined && mode;
    let expected=token.frame==frame && token.now==now && live
        && first.stamp(now,id)==Some(token.stamp) && (cacheable || grant)
        && second.stamp(now,id)==Some(token.stamp);
    assert_eq!(token.valid(&cx),expected,"layout validity reused stale context or policy");
}
#[kani::proof]
fn unchanged_layout_data_is_reusable_without_consuming_authority() {
    let now=crate::Instant(kani::any());let frame:u64=kani::any();let revision:u64=kani::any();let until:u64=kani::any();
    kani::assume(revision % 2 == 0 && now.0 < u128::from(until));
    let id=SessionId(kani::any());let state=Read{revoked:false,before:revision,after:revision,until};
    let access=access::Access::new(now,state,state,id,true,false,true,kani::any(),true);
    let cx=Cx{now,frame,access:&access};
    let token=LayoutToken::new(PermissionBinding::new(0u64),(id,revision),&cx).expect("live layout rejected");
    assert!(token.valid(&cx) && token.valid(&cx),"layout data was consumed or rejected while unchanged");
}
#[kani::proof]
fn custom_policy_is_not_cached_without_an_issuer_revision() {
    let now=crate::Instant(kani::any());let until:u64=kani::any();let revision:u64=kani::any();
    kani::assume(now.0 < u128::from(until) && revision % 2 == 0);
    let state=Read{revoked:false,before:revision,after:revision,until};let id=SessionId(kani::any());
    let mut access=access::Access::new(now,state,state,id,true,false,true,true,true);
    let frame=kani::any();
    let token=LayoutToken::new(PermissionBinding::new(0u64),(id,revision),&Cx{now,frame,access:&access}).unwrap();
    access.grant=false;
    assert!(!token.valid(&Cx{now,frame,access:&access}),"custom policy denial reused cached layout");
}
#[kani::proof]
fn issuer_faults_never_become_layout_authority() {
    let now=crate::Instant(kani::any());let first=Read::any();let second=Read::any();let id=SessionId(kani::any());
    let present:bool=kani::any();let quarantined:bool=kani::any();let mode:bool=kani::any();
    let access=access::Access::new(now,first,second,id,present,quarantined,mode,kani::any(),kani::any());
    let expected=if present && !quarantined && mode {first.stamp(now,id)} else {None};
    assert_eq!(access.authority_stamp(),expected,"issuer fault exposed layout authority");
}
'''

# Harnesses are nested inside the actual private module to inspect its binding.
CLIENT = 'use crate::{access, Read};\n' + CLIENT


def generate(sources):
    token = sources[TOKEN]
    require(token.count('use std::time::Instant;') == 1, 'Unsupported layout time import')
    token = token.replace('use std::time::Instant;', 'use crate::Instant;')
    substitutions = {
        'RULES': str(ROOT / 'crates/fairing/src/access/session_rules.rs'),
        'BIND_NEW': function(sources['crates/fairing/src/access/protected.rs'], 'pub fn new(required: P)'),
        'BIND_GATE': function(sources['crates/fairing/src/access/protected.rs'], 'pub fn gate(&self)'),
        'BIND_ALLOWS': function(sources['crates/fairing/src/access/protected.rs'], 'pub fn allows(&self, cx:'),
        'ID': function(sources[SESSION].split('impl SessionToken {',1)[1], 'pub fn id(&self)'),
        'SNAPSHOT': function(sources[SESSION], 'pub fn snapshot(&self'),
        'REVISION': function(sources[SESSION], 'pub fn revision(&self)'),
        'AUTHORITY': function(sources[ACCESS], 'pub(crate) fn authority_stamp(&self)'),
        'CACHEABLE': function(sources[ACCESS], 'pub(crate) fn layout_permissions_cacheable(&self)'),
        'TOKEN_MODE': function(sources[ACCESS], 'pub fn uses_session_tokens(&self)'),
        'TOKEN': token, 'CLIENT': CLIENT,
    }
    for key, marker in [('CX_FRAME','layout_frame'),('CX_CACHEABLE','layout_permissions_cacheable'),
                        ('CX_AUTHORITY','authority_stamp'),('CX_MODE','token_mode'),('CX_ALLOWS','allows_gate')]:
        substitutions[key] = function(sources[CX], 'pub(crate) fn '+marker+'(')
    source = SUPPORT
    for name, value in substitutions.items():
        source = source.replace('@'+name+'@', value)
    require(not re.search(r'@[A-Z_]+@', source), 'Unexpanded layout proof source')
    return source


CONTROLS = [
    ('omit-frame', TOKEN, 'self.frame == cx.layout_frame()', 'true', 'layout validity reused stale context or policy'),
    ('omit-time', TOKEN, 'self.now == cx.now', 'true', 'layout validity reused stale context or policy'),
    ('omit-first-stamp', TOKEN, 'cx.authority_stamp() == Some(self.stamp)\n            && (', 'true\n            && (', 'layout validity reused stale context or policy'),
    ('omit-final-stamp', TOKEN, '&& cx.authority_stamp() == Some(self.stamp)\n    }', '&& true\n    }', 'layout validity reused stale context or policy'),
    ('cache-custom-policy', TOKEN, '(self.cacheable || self.binding.allows(cx))', 'true', 'custom policy denial reused cached layout'),
    ('wrong-mint-frame', TOKEN, 'frame: cx.layout_frame(),', 'frame: 0,', 'layout mint changed its data binding'),
    ('omit-mint-final-stamp', TOKEN, '|| cx.authority_stamp() != Some(stamp)\n        {', '|| false\n        {', 'layout mint admitted an invalid observation'),
    ('cache-policy-flag', ACCESS, 'self.token_sessions && !self.token_policy', 'self.token_sessions', 'layout mint changed its data binding'),
    ('revoked-snapshot-is-live', SESSION, 'Observation::Revoked => Err(SessionReadError::Revoked),', 'Observation::Revoked => Ok(SessionSnapshot { token:self, revision:0, now:elapsed }),', 'issuer fault exposed layout authority'),
    ('elapsed-overflow-wraps', SESSION,
     "pub fn snapshot(&self, now: Instant) -> Result<SessionSnapshot<'_>, SessionReadError> {\n        let elapsed = u64::try_from(now.saturating_duration_since(self.0.origin).as_nanos())\n            .unwrap_or(u64::MAX);",
     "pub fn snapshot(&self, now: Instant) -> Result<SessionSnapshot<'_>, SessionReadError> {\n        let elapsed = u64::try_from(now.saturating_duration_since(self.0.origin).as_nanos())\n            .unwrap_or(0);", 'issuer fault exposed layout authority'),

]


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    receipt=OUT/'proof.json';receipt.unlink(missing_ok=True)
    paths=[ROOT/name for name in LAYOUT_INPUTS]
    def hashes():return {str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    before=hashes();sources={str(p.relative_to(ROOT)):p.read_text() for p in paths}
    kani=install.tool('kani');channel=install.PINS['kani_toolchain']
    sysroot=Path(subprocess.check_output(['rustup','run',channel,'rustc','--print','sysroot'],text=True).strip())
    env=dict(os.environ,RUSTUP_TOOLCHAIN=channel)
    env['PATH']=str(kani.parent)+os.pathsep+env.get('PATH','')
    env['LD_LIBRARY_PATH']=os.pathsep.join([str(sysroot/'lib'),env.get('LD_LIBRARY_PATH','')])
    def execute(label, inputs):
        path=OUT/(label+'.rs');path.write_text(generate(inputs))
        result=subprocess.run([str(kani),str(path)],cwd=ROOT,env=env,capture_output=True,text=True)
        path.with_suffix('.log').write_text(result.stdout+result.stderr)
        return result
    result=execute('production',sources)
    require(result.returncode==0 and '5 successfully verified harnesses, 0 failures, 5 total' in result.stdout+result.stderr,
            (result.stdout+result.stderr)[-12000:])
    print('layout token: five production harnesses verified',flush=True)
    controls=[]
    for label,file,old,new,assertion in CONTROLS:
        require(sources[file].count(old)==1, ('Ambiguous layout mutation',label))
        result=execute(label,sources|{file:sources[file].replace(old,new,1)})
        content=result.stdout+result.stderr
        failed=re.findall(r'Status: FAILURE\n\s*- Description: ([^\n]*)',content)
        require(result.returncode and any(text.strip('"')==assertion for text in failed),
                'Layout mutation did not fail its security assertion: '+label+'\n'+content[-3000:])
        controls.append({'case':label,'status':'security-assertion-rejected'})
        print(label+': security assertion rejected',flush=True)
    require(hashes()==before,'Layout proof inputs changed during verification')
    receipt.write_text(json.dumps({'status':'production-layout-token-verified','inputs':before,'harnesses':5,'controls':controls,
        'scope':'Unchanged layout new/valid/gate, binding, context-observer, Access stamp/cacheability and session snapshot/revision/identity bodies. Instant equality uses symbolic ticks; elapsed observations cover u64. Atomic schedules are abstract observations through the production session_rules::observe decision, not a new concurrent-storage refinement proof. Permission decisions obey the existing gate/issuer observation contract; custom policy outcomes may change. Host frame/time observations and immutable Access configuration within one layout remain premises. Compiled caller guards are checked separately.'},indent=2)+'\n')
    print('production-layout-token-verified',flush=True)


if __name__=='__main__':main()
