# Releasing Oracy

Audience: the release operator cutting an Oracy repository release. This
document assumes access to the repository, GitHub, the backend container
runtime, and the deployment host.

## Release Identity

Oracy uses one repository tag for all release artifacts. The tag is
`vX.Y.Z`; backend `backend/Cargo.toml` version is `X.Y.Z`; client
`client/pubspec.yaml` release version is `X.Y.Z` with a platform build suffix
such as `+1`.

The tag is the source-truth identity. Artifacts built from the tag must report
that identity:

- `oracy-backend --version` reports `oracy-backend X.Y.Z`.
- Android release manifests derive `versionName` and `versionCode` from
  `client/pubspec.yaml`.
- Backend container images are tagged with the same `X.Y.Z` release identity
  and run the backend binary that reports that version.

## Pre-Release Gate

A releasable commit is on `main`, up to date with `origin/main`, and has a
clean working tree. `--allow-dirty` is not part of the release path.

Before tagging:

```sh
git checkout main
git pull --ff-only
git status --short
./scripts/release-check release "vX.Y.Z"
```

The release check must target the tag being cut. It verifies that source
versions match `X.Y.Z`, `## Unreleased` is present and empty, and the release
notes live under `## [X.Y.Z] - YYYY-MM-DD`.

## Atomic Release Operation

A release is one conceptual operation even when the shell uses more than one
command: changelog rollup, source-version verification, annotated tag, tag
push, and GitHub Release publication describe the same release boundary.

For `vX.Y.Z`:

```sh
git tag -a "vX.Y.Z" -m "Oracy vX.Y.Z" -m "Release date: YYYY-MM-DD" -m "Source: CHANGELOG.md [X.Y.Z]"
git push origin "vX.Y.Z"
```

The tag must point at the commit whose source files already satisfy the release
metadata invariants. If a published tag points at a commit that violates those
invariants, the tag is invalid; delete and re-cut the release rather than
amending the tagged commit.

## Post-Release Gate

The tag push runs the release workflow. That workflow verifies the annotated
tag, builds the backend binary, builds a local backend container image tagged
with the git tag, verifies both artifact version reports, extracts release
notes from `CHANGELOG.md`, and publishes the GitHub Release.

Operators still own deployment artifacts and host state. Build or promote the
deployment image from the release tag, tag it with the release identity, update
the deployment reference to that immutable tag, and validate the live service.
Registry publishing is not owned by this repository yet.

Manual GitHub Release creation, when needed after a workflow failure, uses the
same notes source:

```sh
./scripts/release-check notes "vX.Y.Z" > /tmp/oracy-release-notes.md
gh release create "vX.Y.Z" --title "Oracy vX.Y.Z" --notes-file /tmp/oracy-release-notes.md
```

## Tesserine Adaptation

Tesserine ADR-0006, "Release Discipline for Cargo-Workspace Repos", binds the
principles Oracy follows: clean tree, annotated tag, version metadata before
tagging, changelog rollup at the tagged commit, and a mechanically verifiable
version invariant. Oracy adapts those principles without adopting
`cargo-release`, because `backend/` is a single crate and the repository also
contains Flutter client and container artifacts.

Tesserine ADR-0010, "Deployment Release Candidates", applies when Oracy starts
cutting release candidates. Stable releases use `vX.Y.Z`; future release
candidates use immutable `vX.Y.Z-rc.N` tags and must not deploy from mutable
branch refs. Oracy does not ship release candidates for `v0.1.0`.
