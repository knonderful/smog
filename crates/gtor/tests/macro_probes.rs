//! Compile-time probes for the code-review findings against `gtor-macro`. Each `compile_fail`
//! case is a generator that a user could reasonably write; the pinned `.stderr` files record
//! why the macro rejects it today.

#[test]
fn macro_probes() {
    let t = trybuild::TestCases::new();
    t.pass("tests/macro_probes/control_single_lifetime.rs");
    t.compile_fail("tests/macro_probes/f1_escaped_context_awaited_in_other_generator.rs");
    t.compile_fail("tests/macro_probes/f1_escaped_context_writes_through_foreign_waker.rs");
    t.compile_fail("tests/macro_probes/f4_generic_type_param.rs");
    t.compile_fail("tests/macro_probes/f4_two_elided_lifetimes.rs");
    t.compile_fail("tests/macro_probes/f4_nested_lifetime.rs");
    t.compile_fail("tests/macro_probes/f6_yield_in_match_arm.rs");
    t.compile_fail("tests/macro_probes/f7_ctx_shadowing.rs");
}
