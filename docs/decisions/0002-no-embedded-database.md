# No embedded database — RAM plus files on the SSD

Statistics and the query log use fixed-size in-RAM structures plus append-only
file segments and periodic snapshots on the `/data` volume. No SQLite, no
embedded KV store. Reasons: the RAM/latency budgets leave no room for a storage
engine's overhead and write amplification (Pi-hole's FTL database is its
best-known performance complaint), and our access patterns — bounded ring,
time-ordered scan, age/size pruning — map directly onto flat segments.

## Revisit criteria

Reopen this decision if the product ever needs ad-hoc querying across long
histories (arbitrary filters over months of data) or multi-writer access —
those are database problems, and bolting them onto flat files would be worse
than adopting one.
