# tui-monitor — feed viewport / pause review

## Summary

Read-only review of the working-tree diff (`app.rs`, `models/mod.rs`, `ui/mod.rs`,
`ui/queries.rs`, +660/−38), plus one test added on request.

The change splits the feed's single `feed_row` into a viewport (`feed_top`) and a
selection (`feed_row`). The wheel moves the viewport only; the arrow/page keys
move the selection and drag the viewport the minimum distance needed to keep it
on screen; an open overlay swallows navigation keys and the wheel. `render`
clamps both against the current panel and hands them back, so a resize or a ring
eviction cannot leave either out of range.

No DNS hot-path impact — `fah-tui-monitor` is a separate binary depending only on
`fah-model` (L1). Gates clean: `cargo fmt --check`, `cargo clippy --all-targets
-D warnings`, `cargo test` → **152 passed**.

F3 was fixed during the review (see §F3 fix). F1/F2 are accepted as intentional
consequences of the live, index-based feed. F4/F5/F7 are deferred.

Verdict: **PASS WITH DEFERRED FINDINGS**.

## Decisions

- Viewport and selection are both **feed indices**, not item identities. The
  owner explicitly rejected arrival compensation / anchor / snapshot state; F1
  and F2 below record the consequence rather than propose a fix.
- `render` is the only code that knows the panel geometry, so it owns clamping
  and writes the clamped pair back into `UiState`. `App::visible_rows()`
  therefore reads the *previous* frame's `regions`.
- The wheel acts only while the cursor is over the feed (previously anywhere).
- `*table_state.offset_mut() = 0` each frame: ratatui's own scroll is disabled
  and the widget is handed only the window.

## Findings

| # | Sev | Where | Finding |
| - | --- | ----- | ------- |
| F1 | Major (accepted) | [queries.rs:53-58](../../../tui-monitor/src/ui/queries.rs#L53-L58) | Index-anchored viewport: `push_query` pushes to the front, so a held `feed_top` shows different rows after every arrival. At `top=10` the window shows `d29…` before 7 arrivals and `d36…` after — content still slides under a "paused" viewport, delayed by `top` rows. Owner ruled out arrival compensation; recorded, not to be fixed. |
| F2 | Major (accepted) | [app.rs:186-190](../../../tui-monitor/src/app.rs#L186-L190) | Same for the selection: `feed_row` points at a different query after arrivals, so the highlight drifts. An **open overlay is safe** — `select()` clones the item, as its doc comment states. |
| F3 | Medium — **fixed** | [app.rs:91-92](../../../tui-monitor/src/app.rs#L91-L92) | `Home` moved the viewport only, leaving the selection off screen; `Enter` then opened an invisible row and the next `Up`/`Down` yanked the viewport back to it. `End` was bound to nothing. Fixed below. |
| F4 | Low | [app.rs:157-162](../../../tui-monitor/src/app.rs#L157-L162) | `visible_rows()` returns `1` before the first draw and otherwise the previous frame's height. A key press before the first draw collapses `feed_top` onto `feed_row`; the next `render` clamps it. One-frame lag after a resize, self-correcting. |
| F5 | Low | [ui/mod.rs:74-75](../../../tui-monitor/src/ui/mod.rs#L74-L75) | `draw` mutates the state it was handed (`ui.feed_top`, `ui.feed_row` from `Feed`). Pragmatic — only the renderer knows the area — but it makes UI state a function of the last frame's geometry, and the clamping policy lives in the view. |
| F6 | Low | [queries.rs:163-167](../../../tui-monitor/src/ui/queries.rs#L163-L167) | `row_at` bounds the click by `data_rows(area)`, not by how many rows the feed actually filled, so a click on a blank row below a short feed returns `Some(index)` past the end. `select()` no-ops via `queries.get()`. Harmless, silent. |
| F7 | Nit | [app.rs:122](../../../tui-monitor/src/app.rs#L122) | `over_queries` guards `!behind`, but the left-click path is already inside the `else` of `popup.is_some()`. Redundant condition. |
| F8 | Nit | [app.rs:150-151](../../../tui-monitor/src/app.rs#L150-L151) | Behaviour change outside the stated scope, intended and tested: the wheel over the header/footer/chart no longer scrolls the feed. |

### F3 fix

Three edits in [app.rs](../../../tui-monitor/src/app.rs), no new state, no
arrival compensation, no anchors:

| Change | Effect |
| ------ | ------ |
| `jump_feed(row)` extracted from `move_feed`; `Home` → `jump_feed(0)`, `End` → `jump_feed(usize::MAX)` | Both keys move the selection, and `window_top` pulls the viewport with it. `End` on a feed shorter than the panel leaves `feed_top = 0`. |
| `row_in_view()` clamps `feed_row` into `[feed_top, feed_top + visible)` | An arrow key after a wheel scroll steps **inside** the window on screen instead of dragging it back to the row the wheel left behind. |
| `toggle_popup` selects `row_in_view()` | `Enter` always opens a row the window is showing. |

Invariants now held: after `Home`, `End`, any arrow/page key, or `Enter`, the
selection is inside the viewport; the wheel and a click still leave the
selection and the viewport independent.

No races, deadlocks, task leaks or panic paths introduced. Every new arithmetic
path is `saturating_*`/`min`; `render`'s early return for a sub-2×2 rect still
returns clamped state, covered by
`a_panel_too_small_to_draw_hands_its_state_back_clamped`.

## Measurements

Per-frame feed work, default `[ui] feed_rows = 200`, panel ~30 data rows:

| | Before | After |
| - | ------ | ----- |
| `Row`s built per redraw | 200 (whole ring) | ≤ `visible` (~30) |
| Allocations per row | 1 `Vec<Cell>` (7) + 1 `String` (`millis`) + 1 `String` only for an unnamed client | unchanged |
| Retained UI state | — | +1 `usize` (`feed_top`) |

Inspected the ~30-rows/frame path as requested: the only per-row allocations are
the `Vec<Cell>` and `format!` inside `millis()`; the client label borrows
whenever the router has named the client. At the configured redraw cadence this
is not worth redesigning — the change already cut the per-frame row work ~6–7×
by rendering the window instead of the ring.

## Tests

| Requirement | Test | Status |
| ----------- | ---- | ------ |
| Viewport holds its feed position across arrivals | `ui::tests::arrivals_do_not_pull_a_scrolled_viewport_back_to_the_newest_row` | already present, passes |
| Bounded feed: eviction clamps, no jump, no panic | `ui::tests::evicting_the_oldest_rows_clamps_the_viewport` | already present, passes |
| Selection independent of the viewport (wheel, click) | `app::tests::moving_the_viewport_never_moves_the_selection` | **added** |
| `Home`/`End` move both, selection ends on screen | `home_and_end_carry_the_selection_with_the_viewport` | **added** |
| `End` on an empty feed and on a feed shorter than the panel | `home_and_end_hold_still_on_an_empty_feed`, `a_feed_shorter_than_the_panel_keeps_end_at_the_top` | **added** |
| An arrow key after a wheel scroll does not yank the viewport | `an_arrow_key_steps_inside_the_window_it_was_scrolled_to` | **added** |
| `Enter` opens a row the window is showing | `the_overlay_opens_a_row_the_window_is_showing` | **added** |
| Selection with the viewport following (`Up`/`Down`, `PageUp`/`PageDown`) | `the_arrow_keys_move_the_selection_and_the_viewport_follows_it`, `a_page_down_stops_at_the_oldest_row_the_feed_holds` | present, passes |
| `Enter` opens the selection, not the window top | `the_overlay_opens_the_selected_row_not_the_top_of_the_window` | present, passes |
| Overlay swallows keys and wheel | `an_open_overlay_swallows_the_navigation_keys`, `an_open_overlay_swallows_the_wheel` | present, passes |

Gaps:

- No test pins *which item* sits at a paused viewport after arrivals — the
  existing test asserts only the index and that the newest arrival stays off
  screen. Encoding F1 would make the accepted limitation explicit.
- No test for a key press before the first draw (F4).
- The size sweeps (`1..=60 × 1..=30`, `usize::MAX` offsets) are deterministic —
  no timing dependence anywhere in the added tests.

## Files changed

| File | Change |
| ---- | ------ |
| `tui-monitor/src/app.rs` | `scroll_feed` split into `move_feed`/`scroll_view`; `!behind` guards; `row_at` takes `feed_top`; test module. This review added `jump_feed`/`row_in_view`, the `End` binding and 6 tests |
| `tui-monitor/src/ui/mod.rs` | `UiState::feed_top`; `draw` writes the clamped `Feed` back; feed tests |
| `tui-monitor/src/ui/queries.rs` | `render` windows the ring and returns `Feed`; `visible_rows`/`max_top`/`window_top`; `row_at` bounds the click |
| `tui-monitor/src/models/mod.rs` | `fixtures::query(index)` |

## Remaining TODOs

- F4/F5/F7 deferred, non-blocking.
- F1/F2 accepted, not open — reopen only if the owner revisits the live,
  index-based model.
- No key hints are drawn anywhere in the UI, so the new `End` binding is
  discoverable only from the source. Out of scope for this change.
