"""A sharded harness's share of its cases.

A harness that declares PARITY_SHARDS = n runs as n entries in tools/parity.py, which sets PARITY_SHARD=i/n for
each. Outside the runner PARITY_SHARD is unset and a harness runs everything. Kept out of lab.py: every harness
imports lab.py, so a change there reruns them all.
"""
import os


def parse(spec):
    """Return the 1-based index and shard count."""
    index, count = map(int, spec.split("/"))
    if not 1 <= index <= count:
        raise ValueError(f"Invalid PARITY_SHARD {spec!r}. Expected i/n with 1 <= i <= n.")
    return index, count


def shard(items):
    """Every n-th item from the i-th, or all of them outside the runner."""
    spec = os.environ.get("PARITY_SHARD")
    if not spec:
        return items
    index, count = parse(spec)
    return items[index - 1::count]


def label():
    """A directory name for this share ("2-4", or "all"): shards run at once and must not share outputs."""
    spec = os.environ.get("PARITY_SHARD")
    if spec:
        parse(spec)
    return "all" if spec is None else spec.replace("/", "-")
