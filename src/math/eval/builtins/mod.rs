//! Built-in function implementations, split from the `eval_expr` dispatcher.
//!
//! Each function mirrors one arm of the old `match name.as_str()` block over
//! evaluated arguments. Pure numeric builtins take `(name, args)`; those that
//! reconcile units or defer to quantity arithmetic additionally take `&Context`.

pub mod finance;
pub mod logic;
pub mod math;
pub mod stats;
pub mod trig;
pub mod vector;
