# Exit 0 when the `verify-bundle` job block declares `needs:`.
#
# Expects the job key at two-space indent. The caller (scripts/ci-needs-guard.sh)
# normalises CRLF and has already asserted that the job exists, so "not found"
# is reported there rather than being indistinguishable from "no violation".
$0 ~ /^  "?verify-bundle"?:/ { in_job = 1; next }
in_job && $0 ~ /^  [A-Za-z0-9_-]+:/ { in_job = 0 }
in_job && $0 ~ /^    needs:/ { found = 1 }
END { exit !found }
