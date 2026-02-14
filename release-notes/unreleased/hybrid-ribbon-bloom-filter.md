# Release Notes: Switch default bloom filter to hybrid ribbon

## Behavioral Change

### What Changed
The default RocksDB bloom filter type has been changed from standard bloom filters
to **hybrid ribbon** filters. With this change, SST files flushed to L0 continue to
use standard bloom filters, while SST files produced by compaction (L1+) use ribbon
filters instead.

### Why This Matters
Ribbon filters achieve the same false-positive rate as standard bloom filters while
using ~30% less memory. Because filter blocks are cached in the block cache
(`cache_index_and_filter_blocks = true`), smaller filters free up cache space for
data blocks, improving overall read throughput under memory pressure.

The higher CPU cost of constructing ribbon filters only applies during background
compactions and is negligible in practice.

### Impact on Users
- **No action required.** The change is fully transparent — RocksDB can read both
  bloom and ribbon filter blocks regardless of which policy was used to create them,
  so existing SST files continue to work without rewrite.
- Filter memory usage will gradually decrease as new compactions replace old SST
  files with ones containing ribbon filters.

### Migration Guidance
To revert to the previous behavior, set the following in your Restate configuration:

```toml
[worker.storage.rocksdb]
rocksdb-bloom-filter-type = "bloom"
```
