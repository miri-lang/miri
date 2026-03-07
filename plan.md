1. **Optimize capacity allocation in the Type Checker:**
   - Pre-allocate vectors and hashmaps using `::with_capacity()` in frequently called Type Checker functions to eliminate needless heap allocations.
   - Specifically target areas where the required capacity is immediately available:
     - `src/type_checker/expressions/calls.rs`: `positional_args` and `named_args` using `args.len()`.
     - `src/type_checker/expressions/collections.rs`: `element_types` using `elements.len()`.
     - `src/type_checker/expressions/types.rs`: `new_params` using `func_data.params.len()`.
     - `src/type_checker/expressions/access.rs`: `substituted_variant_types` using `variant_types.len()`.
2. **Complete pre-commit steps to ensure proper testing, verification, review, and reflection are done.**
3. **Submit the change.**
