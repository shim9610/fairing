// Expected to fail with ferrocene::unvalidated. This checks that the lint runs.
#![deny(ferrocene::unvalidated)]

fn ordinary_function() {}

fn main() {
    ordinary_function();
}
