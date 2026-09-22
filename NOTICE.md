# Notices and attribution

Modern Monitor Switcher is licensed under the Mozilla Public License 2.0.
See [LICENSE](LICENSE) for the full text.

## MonitorSwitcher

This project is a ground-up rewrite, but it owes its core technique to
**MonitorSwitcher** by **Martin Krämer**:

- <https://sourceforge.net/projects/monitorswitcher>
- Also licensed under the Mozilla Public License 2.0.

Specifically, the strategies in [`msw-core/src/apply.rs`](msw-core/src/apply.rs)
for re-matching adapter LUIDs when restoring a saved configuration are ported
from that project. Windows reassigns adapter identifiers on every boot, so a
saved display configuration cannot simply be replayed; the original accumulated
its fallback strategies over years of bug reports against real hardware, and
discarding that knowledge to start from scratch would have meant rediscovering
the same failures.

This project is released under the same licence in acknowledgement of that.

The rewrite is not a translation. The Rust implementation adds validation
before applying (so a rejected strategy costs no screen flicker), refuses to
match monitors on an ambiguous identity, and replaced the XML profile format
with JSON.
