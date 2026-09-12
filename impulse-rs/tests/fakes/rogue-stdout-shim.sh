#!/usr/bin/env bash
# Rogue-output shim for the P1 regression test (review round 2 on PR #54):
# ignores every argument (mirroring how a compromised "internal-pdf-text"
# child, or a `current_exe()`/PATH-tampering scenario pointing at something
# else entirely, might behave) and just writes far more than any
# reasonable character-budget-derived stdout cap, forever, until killed.
#
# Proves the PARENT bounds its own read of a child's stdout
# (ion_repl::tool_document::read_capped) rather than trusting
# child.wait_with_output()'s unbounded internal buffer -- P1 found that a
# rogue child streaming ~1 GiB of output drove the parent to ~3.2 GB RSS
# and produced an "accepted" multi-gigabyte-character document.
exec head -c 1073741824 /dev/zero
