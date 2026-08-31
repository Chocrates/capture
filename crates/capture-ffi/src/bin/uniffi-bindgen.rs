//! Generates the Kotlin the phone calls.
//!
//! A binary inside this crate rather than an installed tool, because the
//! generator has to be the same version as the runtime it generates against —
//! and a version skew between them produces Kotlin that compiles and then
//! fails at the first call, which is the worst place to find out.

fn main() {
    uniffi::uniffi_bindgen_main()
}
