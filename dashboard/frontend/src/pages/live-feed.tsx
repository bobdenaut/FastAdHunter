import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import type { QueryEvent } from '../api/types';
import { Card } from '../components/card';
import { EmptyState } from '../components/empty-state';
import type { PageProps } from '../router/routes';
import { socket } from '../services';
import { ContentHeader } from '../shell/content-header';
import { clockLabel } from '../time';
import { Detail, FeedVerdict } from './live-feed/detail';
import {
  EMPTY_FILTERS,
  KINDS,
  VERDICTS,
  applyFilters,
  isFiltered,
  type FeedFilters,
} from './live-feed/filters';
import {
  FeedBuffer,
  matchesNarrow,
  observeNarrow,
  ringCapacity,
} from './live-feed/ring';

/**
 * The rows drawn at once. The ring holds 500 (200 on a phone) and drawing all
 * of them costs a table row per held event on every flush; a page is what is
 * actually read.
 *
 * **Page 1 is the newest**, and new events arrive into it. A later page is a
 * window over rows that are still moving — the ring keeps dropping its oldest —
 * so Pause is what makes paging back stable, and the page says so.
 */
const PAGE_SIZES = [50, 100, 200] as const;

/**
 * Every query and request as it is decided.
 *
 * **This is a live tail, not a query log.** FastAdHunter keeps no per-query
 * record: the feed starts empty when the page opens, holds a fixed ring in this
 * tab, and retains nothing when you leave. There is no history to search, by
 * design — no per-query store means no per-query memory growth.
 *
 * **It is also the only screen that subscribes to `query`**, and the
 * subscription is the route table's, acquired on mount and released on unmount
 * by the shell's route transition. Leaving empties the union, which closes the
 * socket — so the household's whole per-query feed stops arriving at a phone
 * that has moved on, and the engine stops doing per-query publish work for a
 * page nobody is looking at (`main.rs` `hub.has_query_subscribers()`).
 */
export function LiveFeed(_props: PageProps) {
  const [rows, setRows] = useState<readonly QueryEvent[]>([]);
  /** The oldest held row's sequence number, from the ring — a row's identity. */
  const [firstSeq, setFirstSeq] = useState(0);
  const [filters, setFilters] = useState<FeedFilters>(EMPTY_FILTERS);
  const [paused, setPaused] = useState(false);
  const [showFilters, setShowFilters] = useState(false);
  const [held, setHeld] = useState(0);
  const [pageSize, setPageSize] = useState<number>(PAGE_SIZES[0]);
  const [page, setPage] = useState(0);
  const pausedRef = useRef(false);
  pausedRef.current = paused;

  // **The bound is sampled once; the layout is not.** The ring's capacity is a
  // memory bound and stays at its mount-time value (X6) — a rotation mid-visit
  // does not reallocate it, and the line on the page states that rather than
  // hides it. The layout has to follow the viewport instead: this page builds
  // one tree, and `display: none` is what hides the other, so a window dragged
  // across the breakpoint would otherwise leave the pager counting rows over an
  // empty body.
  const [openedNarrow] = useState(matchesNarrow);
  const capacity = ringCapacity(() => openedNarrow);
  const [narrow, setNarrow] = useState(openedNarrow);
  useEffect(() => observeNarrow(setNarrow), []);

  const buffer = useMemo(
    () =>
      new FeedBuffer<QueryEvent>(capacity, (items, firstSequence) => {
        setHeld(items.length);
        // Pause freezes the **rendered snapshot** only: the ring behind it keeps
        // filling and keeps dropping its oldest, so resuming shows the last N
        // rather than a gap. The sequence moves with the snapshot it numbers.
        if (!pausedRef.current) {
          setRows(items);
          setFirstSeq(firstSequence);
        }
      }),
    [capacity],
  );

  useEffect(() => () => buffer.dispose(), [buffer]);

  useEffect(
    () =>
      socket.on('query', (data) => {
        buffer.push(data as unknown as QueryEvent);
      }),
    [buffer],
  );

  /**
   * **Newest first.** The ring hands its rows back oldest-first — that is the
   * order a buffer has — and a tail is read from the top, so the reversal
   * happens once here, at the render boundary, rather than in the ring or in
   * the filters.
   */
  // Each row carries the sequence number it was pushed at, so a key survives
  // the reverse, the filter and the pager — all three of which move a row's
  // index while leaving the event it holds the same one.
  const visible = useMemo(
    () =>
      applyFilters(
        rows.map((row, index) => ({ row, seq: firstSeq + index })),
        filters,
        (entry) => entry.row,
      ).reverse(),
    [rows, firstSeq, filters],
  );

  // Clamped rather than corrected in an effect: the row count shrinks on its
  // own as the ring drops its oldest, and a page index chasing that in state
  // would render one frame past the end each time.
  const pageCount = Math.max(1, Math.ceil(visible.length / pageSize));
  const current = Math.min(page, pageCount - 1);
  const from = current * pageSize;
  const shown = visible.slice(from, from + pageSize);

  const set = (patch: Partial<FeedFilters>) => {
    setFilters((existing) => ({ ...existing, ...patch }));
    // A filter narrows the set under the pager; staying on page 4 of a set
    // that now has one page would show an empty table over a non-empty feed.
    setPage(0);
  };

  return (
    <>
      <ContentHeader
        title="Live Feed"
        context={
          <>
            Every query and request as it is decided, from{' '}
            <span class="mono">WS /api/v1/events</span>
          </>
        }
      />
      <main class="wrap">
        <div class="banner feed-notice" role="status">
          <div>
            This is a live tail, <b>not a query log</b>. FastAdHunter keeps no
            per-query records — the feed starts empty when the page opens, holds
            the last {capacity} rows in this tab, and retains nothing when you
            leave. Filters below apply to those rows only. There is no history to
            search, by design: no per-query store means no per-query memory
            growth.
            <span class="footnote-line">
              Rendering stops while this page is hidden. Switch apps or lock the
              screen and the feed pauses; it resumes on return, having missed
              whatever arrived meanwhile — there is no stored history to backfill
              from. The ring holds {capacity} rows here — the bound is sized to
              the device, 500 on a desktop viewport and 200 on a phone, and
              fixed at the moment this page opened.
            </span>
          </div>
        </div>

        <Card
          title="Filters"
          secondary="applied in the browser, over the rows held here"
          className="feed-filters"
        >
          <div class="feed-controls">
            <div class="feed-chipset">
              <span class="note">verdict</span>
              <span class="chips">
                <Chip
                  label="all"
                  on={filters.verdict === ''}
                  onPick={() => set({ verdict: '' })}
                />
                {VERDICTS.map((verdict) => (
                  <Chip
                    key={verdict}
                    label={verdict}
                    on={filters.verdict === verdict}
                    onPick={() => set({ verdict })}
                  />
                ))}
              </span>
            </div>
            <div class="feed-chipset">
              <span class="note">pipeline</span>
              <span class="chips">
                <Chip
                  label="all"
                  on={filters.kind === ''}
                  onPick={() => set({ kind: '' })}
                />
                {KINDS.map((kind) => (
                  <Chip
                    key={kind}
                    label={kind}
                    on={filters.kind === kind}
                    onPick={() => set({ kind })}
                  />
                ))}
              </span>
            </div>
            <div class={showFilters ? 'feed-text on' : 'feed-text'}>
              <label class="field-label" for="feed-client">
                client
              </label>
              <input
                id="feed-client"
                class="field-input mono"
                type="text"
                placeholder="any client"
                spellcheck={false}
                value={filters.client}
                onInput={(event) =>
                  set({ client: (event.target as HTMLInputElement).value })
                }
              />
              <label class="field-label" for="feed-domain">
                domain contains
              </label>
              <input
                id="feed-domain"
                class="field-input mono"
                type="text"
                placeholder="filter…"
                spellcheck={false}
                value={filters.domain}
                onInput={(event) =>
                  set({ domain: (event.target as HTMLInputElement).value })
                }
              />
            </div>
            <div class="feed-actions">
              <button
                type="button"
                class={paused ? 'btn' : 'btn g'}
                aria-pressed={paused}
                onClick={() => {
                  const next = !paused;
                  setPaused(next);
                  // Resuming shows the ring as it stands now rather than
                  // waiting for the next event on an idle feed.
                  if (!next) {
                    setRows(buffer.items());
                    setFirstSeq(buffer.firstSequence);
                  }
                }}
              >
                {paused ? 'Resume' : 'Pause'}
              </button>
              <button
                type="button"
                class="btn g"
                onClick={() => {
                  buffer.clear();
                  setRows([]);
                  setFirstSeq(buffer.firstSequence);
                  setHeld(0);
                  setPage(0);
                }}
              >
                Clear
              </button>
              <button
                type="button"
                class="btn g feed-filter-toggle"
                aria-expanded={showFilters}
                onClick={() => setShowFilters((open) => !open)}
              >
                Filter…
              </button>
            </div>
          </div>
        </Card>

        <Card
          title="Feed"
          secondary={`${held} / ${capacity} rows held`}
          className="feed-card"
          bodyClass="feed-body"
        >
          {rows.length === 0 ? (
            <EmptyState title="Starts empty">
              Rows appear as the household resolves. Nothing is retained between
              visits — this tab is the whole of it.
            </EmptyState>
          ) : visible.length === 0 ? (
            <EmptyState title="No held row matches these filters">
              {held} {held === 1 ? 'row is' : 'rows are'} held. Filters apply
              only to those.
            </EmptyState>
          ) : (
            <>
              <div class="feed-pager">
                <span class="note">
                  rows {from + 1}–{from + shown.length} of {visible.length}
                  {visible.length !== held && <> matching, {held} held</>} · page{' '}
                  {current + 1} of {pageCount}
                </span>
                <span class="chips feed-page-sizes">
                  {PAGE_SIZES.map((size) => (
                    <button
                      key={size}
                      type="button"
                      class={size === pageSize ? 'chip on' : 'chip'}
                      aria-pressed={size === pageSize}
                      onClick={() => {
                        setPageSize(size);
                        setPage(0);
                      }}
                    >
                      {size}
                    </button>
                  ))}
                </span>
                <span class="feed-page-nav">
                  <button
                    type="button"
                    class="btn g"
                    disabled={current === 0}
                    aria-label="Newer rows"
                    onClick={() => setPage(Math.max(0, current - 1))}
                  >
                    Newer
                  </button>
                  <button
                    type="button"
                    class="btn g"
                    disabled={current >= pageCount - 1}
                    aria-label="Older rows"
                    onClick={() => setPage(Math.min(pageCount - 1, current + 1))}
                  >
                    Older
                  </button>
                </span>
              </div>

              {/* **One tree, not both.** The CSS hides whichever does not
                  belong at this width, but `display: none` is not "out of the
                  tree": every flush was building the nine-column table *and*
                  the card list for every visible row, doubling exactly the
                  per-flush cost the frame coalescing exists to bound. Which one
                  is built follows the breakpoint live — the CSS would otherwise
                  hide the only tree there is the moment the window crosses it —
                  while the ring's bound stays at the value it opened with. */}
              {narrow ? null : (
              <div class="feed-scroll">
                <table class="t feed-table">
                  <thead>
                    <tr>
                      <th>Time</th>
                      <th>Pipe</th>
                      <th>Client</th>
                      <th>Domain / path</th>
                      <th>Verdict</th>
                      <th>Rule</th>
                      <th>List</th>
                      <th>Detail</th>
                      <th class="num">ms</th>
                    </tr>
                  </thead>
                  <tbody>
                    {shown.map(({ row, seq }) => (
                      <tr key={seq}>
                        <td class="mono">{time(row.ts)}</td>
                        <td class="feed-kind">{row.kind}</td>
                        <td>{row.client_name ?? row.client}</td>
                        <td class="mono feed-domain">
                          {row.domain}
                          {row.path !== null && (
                            <span class="feed-path">{row.path}</span>
                          )}
                        </td>
                        <td>
                          <FeedVerdict verdict={row.verdict} />
                        </td>
                        <td class="mono">{row.rule ?? '—'}</td>
                        <td>{row.list ?? '—'}</td>
                        <td>
                          <Detail row={row} />
                        </td>
                        <td class="num mono">{row.duration_ms.toFixed(1)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              )}

              {/* The phone layout, per `MobileLiveFeed.dc.html`: one card per
                  event with a verdict-coloured left border, never the
                  nine-column table. */}
              {!narrow ? null : (
              <div class="feed-cards">
                {shown.map(({ row, seq }) => (
                  <article class={`ev ev-${row.verdict}`} key={seq}>
                    <div class="ev-top">
                      <FeedVerdict verdict={row.verdict} />
                      <span class="feed-kind">{row.kind}</span>
                      <span class="note mono">{time(row.ts)}</span>
                    </div>
                    <div class="mono ev-domain">
                      {row.domain}
                      {row.path !== null && (
                        <span class="feed-path">{row.path}</span>
                      )}
                    </div>
                    <div class="ev-meta note">
                      <span>{row.client_name ?? row.client}</span>
                      {row.rule !== null && (
                        <span class="mono">{row.rule}</span>
                      )}
                      {row.list !== null && <span>{row.list}</span>}
                      <Detail row={row} />
                      <span class="mono">{row.duration_ms.toFixed(1)} ms</span>
                    </div>
                  </article>
                ))}
              </div>
              )}
            </>
          )}
          <p class="note">
            <span class="mono">cached</span> is a marker on a row, not a verdict
            — a cache hit is still a <span class="mono">pass</span>.
            <span class="footnote-line">
              <span class="mono">endpoint N</span> is the answering server&rsquo;s
              index and appears only on a forwarded DNS answer.
            </span>
            <span class="footnote-line">
              A slow tab is disconnected by the engine rather than
              back-pressuring it; the socket reconnects on its own.
            </span>
            {isFiltered(filters) && (
              <span class="footnote-line">
                Showing {visible.length} of {held} held rows.
              </span>
            )}
            {pageCount > 1 && (
              <span class="footnote-line">
                Page 1 is the newest, and new events arrive into it — a page
                further back is a window over rows that are still moving. Pause
                is what holds them still.
              </span>
            )}
          </p>
        </Card>
      </main>
    </>
  );
}

function Chip({
  label,
  on,
  onPick,
}: {
  label: string;
  on: boolean;
  onPick: () => void;
}) {
  return (
    <button
      type="button"
      class={on ? 'chip on' : 'chip'}
      aria-pressed={on}
      onClick={onPick}
    >
      {label}
    </button>
  );
}

/** Browser-local clock time, as every other page prints one. The event's `ts`
 *  is RFC 3339 with milliseconds; the seconds are what a feed is read at. */
function time(ts: string): string {
  const at = Date.parse(ts);
  return Number.isNaN(at) ? ts : clockLabel(at);
}

export default LiveFeed;
