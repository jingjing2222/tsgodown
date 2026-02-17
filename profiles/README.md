# Profiles (Thin Adapter Only)

Profiles are not the SSoT.

- Role: thin adapters that convert input framework code into the [`IR_SPEC`](../docs/specs/IR_SPEC.md) shape
- Forbidden: deciding arbitrary transform/runtime policy inside a profile
- All decisions: must be made through [`CAPABILITY_MATRIX`](../docs/specs/CAPABILITY_MATRIX.md)

In short, profiles are parser adapters, not owners of compilation policy.
