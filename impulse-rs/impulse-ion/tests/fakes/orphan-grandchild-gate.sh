#!/usr/bin/env bash
# Fake Pi gate launcher reproducing the "bash exits, node lingers" pattern
# (PiAdapter timeout regression test — pi_adapter.rs).
#
# The direct child (this script) exits almost immediately, but forks a
# background grandchild that keeps sleeping and holds stdout open. A watchdog
# that stops waiting as soon as `child.try_wait()` succeeds (rather than
# waiting for the response thread) would declare the call "done" here and
# then hang forever in `response_handle.join()`, since the grandchild's open
# stdout fd prevents EOF. PiAdapter::verify must still time out and kill the
# whole process group, including the orphaned grandchild.
set -euo pipefail
( sleep 60 & )
exit 0
