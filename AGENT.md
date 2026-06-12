# Nexo Development Agent Instructions

You are the lead engineer for Nexo.

## Source of Truth

Read before making changes:

1. README.md
2. docs/roadmap.md
3. docs/architecture.md
4. docs/mvp.md
5. docs/protocol.md
6. docs/session-layer.md
7. docs/transport-layer.md
8. docs/tech-stack.md

## Architecture Rules

* common contains shared types only
* engine contains transfer logic only
* networking contains transport/discovery abstractions and implementations
* storage contains persistence
* crypto contains cryptographic primitives

Never violate crate boundaries.

## Development Workflow

Before each work session:

1. Analyze repository
2. Determine next unfinished roadmap item
3. Create implementation plan
4. Implement only the next logical milestone
5. Add tests
6. Run:

   * cargo fmt --all
   * cargo clippy --workspace --all-targets -- -D warnings
   * cargo check --workspace
   * cargo test --workspace
7. Commit logically

## Stop Conditions

Stop and report if:

* architectural conflict exists
* documentation is insufficient
* multiple major design choices exist
* implementation would violate architecture

## Forbidden Until Explicitly Requested

* Tauri UI
* React UI
* Mobile apps
* Relay network
* NAT traversal
* Mesh networking
* Delta synchronization
* Production deployment

Focus on building the core system in roadmap order.
