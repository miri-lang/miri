## Rule

Repairs are classified by safety level to protect you from unintended changes. The `miri fix --apply` command automatically applies only repairs that are safe to apply unconditionally: format-only, behavior-preserving, or local-edit. Repairs classified as api-changing, target-changing, or requires-human-review are refused unless you explicitly approve them with the `--allow-risky` flag.

An api-changing repair modifies a public API surface that other functions and modules observe — changing a module-scope binding from immutable to mutable affects callers. A target-changing repair alters where or how the program runs (GPU residency, target capability, scalar width). A requires-human-review repair is ambiguous or carries sufficient risk that a human should inspect the plan before it applies.

## Before

```
miri fix --apply main.mi
# File declares a module-scope `let counter = 0` that is reassigned in a function.
# The repair would make `counter` mutable, changing a public surface.
# MER_BLD_002: refused repairs require human review (pass --allow-risky to override)
```

## After

```
# Option 1: Review and approve the specific changes
miri fix --apply --allow-risky main.mi

# Option 2: Run --plan first to inspect the changes before applying
miri fix --plan main.mi
# then, after reviewing the proposed repairs, apply them with --allow-risky
miri fix --apply --allow-risky main.mi
```

## Reference

[Build and Command Line](../reference/build.md)
