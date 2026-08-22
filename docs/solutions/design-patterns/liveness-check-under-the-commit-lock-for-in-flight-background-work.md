---
title: Liveness check under the commit lock for in-flight background work
date: 2026-08-22
category: design-patterns
module: fah-rules
problem_type: design_pattern
component: background_job
severity: medium
applies_when:
  - a background task holds an Arc handle to an entry that an API call can delete meanwhile
  - the task ends by writing durable state (a file, a cache) keyed by the entry's id
  - a delete is documented as a guarantee ("204 means the copy is gone") rather than best effort
  - the same id can be re-added while the old task is still running
symptoms:
  - "DELETE answers 204, then a refresh that was already in flight writes the deleted list's cached copy back"
  - a re-added id inherits the old id's baseline because the stale copy reappeared
  - a status map grows an entry for an id that no longer exists, so existence checks keyed on status lie
root_cause: concurrency
resolution_type: code_fix
related_components:
  - fah-rules/lifecycle
  - fah-api/routes
tags:
  - tokio
  - arc-ptr-eq
  - lock-ordering
  - delete-race
  - background-refresh
  - status-map
---

# Liveness check under the commit lock for in-flight background work

## Context

`ListManager::fetch_and_commit` runs with an `Arc<ListEntry>` snapshot, so a
`DELETE /api/v1/lists/{id}` that lands during the fetch cannot stop it. Before
`p2.5-02` the stale commit was documented as harmless ("inert until the next
boot sweep"). `p2.5-02` made `DELETE` + re-add the recovery contract for a
rejected list, and API.md now says a `204` means the baseline is gone — which
the stale commit falsified inside a window the size of one fetch. The `202`
refresh route is background work, so "refresh, then delete" is an ordinary
operator sequence, not a contrived race.

## Guidance

Make the delete and the commit contend on one lock, and have the worker
re-check identity under that lock before it writes:

```rust
let _guard = self.compile_lock.lock().await;
let still_registered = self
    .find(&entry.id)
    .is_some_and(|live| std::ptr::eq(Arc::as_ptr(&live), entry));
if !still_registered {
    return Err(LifecycleError::ListRemoved(entry.id.to_string()));
}
```

`remove_list` takes the same `compile_lock` before it deletes the cached copy
and the entry. Either the worker holds the lock first and the delete removes
what it just wrote, or the delete holds it first and the worker sees a missing
or different `Arc` and bails before parsing. Pointer identity, not id
equality: a re-added id is a new `Arc`, so the old task does not adopt it.

Pair it with an update-only status write so a stale attempt cannot resurrect
a deleted id:

```rust
let Some(entry) = status.get_mut(id) else {
    return;
};
```

`entry(id).or_default()` there had let any failed attempt create a status for
an unknown id, which the API's `status(&id).is_none()` 404 check then trusted.

## Why This Matters

A "harmless" stale write stops being harmless the moment a contract is built
on the delete. The fix is two lines at the seam that already serializes
writes, not a new lock, a generation counter or a cancellation token. It also
shortens the lock hold in the bad case (parse skipped) and costs nothing on
the query path, which never touches these locks.

## When to Apply

- A worker snapshots an `Arc` to something an API can remove, and finishes by
  writing state keyed by that thing's id.
- A delete is documented as a guarantee.
- Re-adding the same id is a supported operation.

Not needed when the stale write is genuinely inert (nothing reads it and the
id cannot come back) — say so in the doc instead.

## Examples

Test shape that pins it (`a_refresh_in_flight_across_delete_and_re_add_never_commits_or_stamps`):
gate the fake server's response with a `tokio::sync::Notify`, spawn the
refresh, yield a few times, `remove_list`, release the gate, then assert
`Err(ListRemoved)`, no cache file, no status entry, and a re-added id with
`compiled == None` and `NeverAttempted`.

## Related

- `docs/code-review/phase2.5/p2.5-02-list-refresh-integrity-review.md`
  (pass 3, P3-1) — the finding and the lock-order argument.
- `docs/solutions/design-patterns/static-dispatch-test-seams-for-hot-path-listener-loops.md`
  — the other gated-I/O test seam in this crate family.
