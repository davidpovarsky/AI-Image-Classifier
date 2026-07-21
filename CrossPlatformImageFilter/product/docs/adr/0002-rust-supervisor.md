# ADR 0002: Rust supervisor

Status: accepted. A Rust service owns all privileged lifecycle and recovery operations through an explicit transactional state machine. This keeps memory-safe control logic separate from inference and presentation.
