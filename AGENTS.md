# Agents

Dynet currently establishes a governed CLI and API boundary. Its product
capabilities are intentionally undecided; do not infer a domain model, runtime,
protocol, storage system, or network behavior from the repository name.

## Repository

- `crates/api` owns the reusable API surface.
- `crates/cli` owns command grammar and user-facing output.
- Dependency direction is `cli -> api`.
- Keep the API independent of CLI presentation concerns.
- Add product vocabulary only when its behavior and ownership have been
  explicitly settled.

Run `plumb doctor .` before and after changing repository shape. Before
landing, run `cargo fmt --all --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`,
`cargo check --locked --workspace --all-targets --release`,
`cargo test --locked --workspace`, and `ectropy .`.

Never work or commit directly in the clean `main` integration checkout after
the initial repository bootstrap. Use a dedicated task branch and land through
the repository guard.
