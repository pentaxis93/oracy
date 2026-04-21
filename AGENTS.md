# Oracy

Study the pentaxis93 commons before changing this project:

- Principles: https://github.com/tesserine/commons

## Project Principles

1. Always greenfield.
   Derive the Rust backend from present requirements and Rust-native design.
   The archived Python implementation is reference material for the domain and
   client reality, not an architectural ancestor.

2. Content is the work.
   Documents earn their existence by carrying substantive decisions. Do not add
   scaffolds, headings, or placeholder files without real content.

3. Everything Earns Its Place.
   v0.1.0 scope is database + transcription + search. A change that does not
   serve one of these does not land in v0.1.0. This applies at every level —
   endpoints, dependencies, directory structure, configuration surface, schema
   fields. PRs that add unearned surface do not land.

4. Spec owns WHAT; implementation owns HOW.
   The spec defines contract-visible behavior: endpoints, request/response
   shapes, status codes, error semantics, auth model, durability commitments.
   The implementation owns everything that does not leak through the contract —
   module layout, persistence details, library choices, internal concurrency,
   vendor selection. Changes to contract-visible behavior update the spec in the
   same commit. Substrate decisions that do not affect the contract do not.

5. Fix-or-file discipline.
   When an agent observes a violation, defect, or inconsistency while working
   on something else, there are exactly three acceptable dispositions:
   a. Fix it immediately (when the fix is quick and does not enlarge the
      current change's blast radius — roughly David Allen's two-minute rule
      from GTD)
   b. File it as an issue for later work
   c. Explicitly name it as a no-action decision with rationale
   "Out of scope" alone is not a disposition. Silent skipping is a violation —
   agents do not have stateful memory, so anything not fixed or filed is lost.
   Every observation gets one of the three dispositions. This discipline is
   also encoded in tesserine/commons ADR-0003.

## Working Stance

- Understand before modifying. Read the real state; do not assume.
- When something seems wrong, say so. Silence is complicity.
- Prefer simplicity. Do not add what is not needed.
- Specify outcomes and constraints, not procedure.
