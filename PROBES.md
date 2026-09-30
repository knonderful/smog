# Review probes

Tests written to verify the findings of the 2026-09-30 code review. All of them use only the
safe, public API of `gtor`.

| Finding | Probe | Command |
|---------|-------|---------|
| 1 `Yield::poll` trusts the waker data pointer | `crates/gtor/tests/review_probes.rs`, `finding_1_*` | `cargo test -p gtor --test review_probes` (debug and `--release`) |
| 2 state pointer derived from `&State` | crate's own `test::test_infinite` under Miri | `cargo +nightly miri test -p gtor --lib` |
| 3 `Clone` on `GeneratorContext` | `review_probes.rs`, `finding_3_*` | as for finding 1 |
| 4 `use<..>` incomplete | `crates/gtor/tests/macro_probes/f4_*.rs` + `.stderr` | `cargo test -p gtor --test macro_probes` |
| 5 MSRV 1.84 too low | pristine checkout without trybuild | `cargo +1.84.1 check -p gtor --tests` |
| 6 `yield_value!` in expression position | `macro_probes/f6_yield_in_match_arm.rs` | as for finding 4 |
| 7 `ctx` hygiene | `macro_probes/f7_ctx_shadowing.rs` | as for finding 4 |
| 9 example package name | none needed | `cargo run -p gtor-macro-example` |

Results on 2026-09-30, rustc 1.98.1 stable, nightly Miri, 1.84.1:

* Finding 1: debug build panics with `Expected state magic value ...` while polling a `Yield`
  under a waker that points at a zero-filled `[u8; 256]` owned by the test. Release build
  returns `Pending` and the buffer is no longer all zero. The second probe awaits an escaped
  `GeneratorContext<String>` inside a `GeneratorContext<u8>` generator and trips the canary.
* Finding 2: Miri, on the unmodified `test_infinite`:
  `Undefined Behavior: trying to retag from <..> for Unique permission ... but that tag only
  grants SharedReadOnly permission`, created at `lib.rs:357` (`state as *const State<Y>`),
  used at `lib.rs:154`.
* Finding 3: both probes panic with
  `BUG: Yield future encountered an existing value in the state.`
* Findings 4, 6, 7: see the pinned `.stderr` files. Headline errors:
  `impl Trait must mention all type parameters in scope in use<...>`,
  `E0106 missing lifetime specifier` + `E0700`, `E0700`,
  `cannot find macro yield_value in this scope`,
  `E0599 no method named yield_value found for type usize`.
* Finding 5: `cargo +1.84.1 check` fails with three `E0658: async closures are unstable`.
* Finding 9: `error: package(s) gtor-macro-example not found in workspace`.
* Finding 8 (duplicated factory body) is a simplification, nothing to test.
