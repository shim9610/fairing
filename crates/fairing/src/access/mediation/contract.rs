//! One declaration supplies runtime adapters and compiler-discovered contracts.
//!
//! There is no verifier-side list of carrier types or protected protocols.
#[allow(dead_code, reason = "consumed by the mediation compiler checker")]
pub(crate) trait Carrier {
    type Payload: ?Sized;
}
#[allow(dead_code, reason = "consumed by the mediation compiler checker")]
pub(crate) trait Declaration {}

macro_rules! protected_contract {
    (carrier $(#[$attr:meta])* $vis:vis struct $name:ident<$p:ident: ?Sized> : $payload:ty {$($fields:tt)*}) => {
        $(#[$attr])* $vis struct $name<$p: ?Sized> {$($fields)*}
        impl<$p: ?Sized> $crate::access::mediation::contract::Carrier for $name<$p> { type Payload = $payload; }
    };
    (carrier $(#[$attr:meta])* $vis:vis struct $name:ident<$p:ident: $bound:path> : $payload:ty {$($fields:tt)*}) => {
        $(#[$attr])* $vis struct $name<$p: $bound> {$($fields)*}
        impl<$p: $bound> $crate::access::mediation::contract::Carrier for $name<$p> { type Payload = $payload; }
    };
    (carrier $(#[$attr:meta])* $vis:vis struct $name:ident<$p:ident: $bound:path, $t:ident> : $payload:ty {$($fields:tt)*}) => {
        $(#[$attr])* $vis struct $name<$p: $bound, $t> {$($fields)*}
        impl<$p: $bound, $t> $crate::access::mediation::contract::Carrier for $name<$p,$t> { type Payload = $payload; }
    };
    (carrier $(#[$attr:meta])* $vis:vis struct $name:ident : $payload:ty {$($fields:tt)*}) => {
        $(#[$attr])* $vis struct $name {$($fields)*}
        impl $crate::access::mediation::contract::Carrier for $name { type Payload = $payload; }
    };
    (binding [] $target:ty) => { impl $target { protected_contract!(@binding_methods); } };
    (binding [$($generics:tt)+] $target:ty) => { impl<$($generics)+> $target { protected_contract!(@binding_methods); } };
    (@binding_methods) => {
        pub(crate) fn required<P: Permission>(mut self,binding:PermissionBinding<P>)->Self {
            self.requirement=Requirement::Required(binding.into_gate());
            self
        }
        pub(crate) fn library_requirement(&mut self,gate:Gate) {
            if matches!(self.requirement,Requirement::Legacy) {self.requirement=Requirement::Required(gate);}
        }
    };
    (operation [$($generics:tt)+] $target:ty, $name:ident($($arg:ident: $ty:ty),*), |$body:ident, $context:ident| $operation:expr) => {
        impl<$($generics)+> $target {
            pub(crate) fn $name(&mut self, $($arg:$ty,)* cx:&mut Cx<'_>) {
                self.invoke(cx, |$body,$context| { $operation; });
            }
        }
    };
    (operation $target:ty, $name:ident($($arg:ident: $ty:ty),*), |$body:ident, $context:ident| $operation:expr) => {
        impl $target {
            pub(crate) fn $name(&mut self, $($arg:$ty,)* cx:&mut Cx<'_>) {
                self.invoke(cx, |$body,$context| { $operation; });
            }
        }
    };
    (screen $name:ident($($arg:ident: $ty:ty),*)) => {
        impl ScreenHandle {
            pub(crate) fn $name(&mut self, $($arg:$ty,)* cx:&mut Cx<'_>) {
                self.invoke(cx, |body,cx| body.$name($($arg,)*cx));
            }
        }
    };
    (screen $name:ident() -> $result:ty, $denied:expr) => {
        impl ScreenHandle {
            pub(crate) fn $name(&mut self,cx:&mut Cx<'_>)->$result {
                self.invoke(cx, |body,cx| body.$name(cx)).unwrap_or($denied)
            }
        }
    };
    (declaration $target:ty => $variant:ident, $field:ident $(, $visibility:ident)?) => {
        protected_contract! { declaration $target {
            fn protect<P: Permission>(mut self,binding:PermissionBinding<P>)->Decl {
                self.gate=Some(binding.gate().clone());
                $(self.$visibility=Visibility::Hidden;)?
                self.$field=self.$field.required(binding);
                Decl::$variant(self)
            }
        }}
    };
    (declaration $target:ty {$($implementation:tt)*}) => {
        impl ProtectedDeclaration for $target {}
        impl $crate::access::mediation::contract::Declaration for $target {}
        impl sealed::Declaration for $target {$($implementation)*}
    };
}
