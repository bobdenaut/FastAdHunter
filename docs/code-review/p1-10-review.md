Results vs PERFORMANCE.md
Budget	Target	Measured	Headroom
Compiled ruleset, 1M	≤ 40 MB	28.3 MiB	1.4×
Blocked query	< 1 ms	1.83 µs	~550×
Verdict + cache hit	< 1 ms	1.87 µs	~535×
Forwarded overhead	< 1 ms	2.44 µs	~410×
Throughput (4 cores)	≥ 10 000 QPS	611 000 QPS	61×
Startup, 1M cached	1–3 s	300 ms	3.3× under goal
RAM steady-state	≤ 128 MB	not measurable	→ p1-11
RSS needs /proc, which Windows doesn't have. I left that row explicitly unmeasured rather than substituting a Windows figure that wouldn't mean anything — the number that counts is the RB5009's, in p1-11.

What shipped
crates/fastadhunter/benches/pipeline.rs — the five budget rows no single crate can answer, in the binary's package since it's the only crate allowed to see every layer. Each bench asserts its own premise (blocked queries never reach the forwarder; the forward-path bench actually forwards) so a broken bench fails instead of reporting a fast meaningless number.

crates/fastadhunter/tests/e2e.rs — spawns the real executable against tempdir volumes and a mock upstream: blocked domain → 0.0.0.0 TTL 10, allowed → upstream answer, the query on the WebSocket, stats + query log, verdict flipping live, key rotation invalidating the old key. 4.5s, fully offline.

Three things you should know
1. The "+159% subdomain regression" was fake — measurement noise, not code. Pinned: 366.8 ns ±0.7, and matcher.rs hasn't been touched since the baseline. I've written the pinning recipe into PERFORMANCE.md §Measuring reliably so this doesn't cost another session.

2. Your instinct about where time goes was right, and I recorded it. The matcher is 3% of a cache-hit query. Both that and the noise finding are saved to memory, and the p1-10 completion note names the real targets in your order — with the caveat that the 3%/97% split is inferred by subtracting separate benches, not measured in one profile. Worth a decomposition bench before acting on it.

3. The e2e test was flaky and I fixed it, not retried it. First full-workspace run failed; second passed. Root cause: bind-a-port-then-release races against the dozens of other tests binding ephemeral sockets in parallel. It now retries with fresh ports only on a genuine port conflict — any other startup failure still fails loudly with the engine log attached. Verified with 5 consecutive full-workspace runs.

Only p1-11 (RB5009 deploy + soak) remains, so phase1 stays in wip. Nothing committed.