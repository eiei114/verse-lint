# Refusal fixtures: inputs the pinned grammar/coverage validation rejects today.
# The sweep asserts both binaries refuse them with identical bytes and exit code.
#
# `refuse-map-type-in-value-position.verse` holds `values := map[K]V := 1`, which
# is a map *type* used where a value is expected: it stays a refusal. The
# `map{...}` literal itself is accepted and is covered by
# `tests/coverage-gaps.rs`, so it is not a fixture here — a fixture in this
# directory that starts succeeding would make the sweep fail for the right
# reason, but the sweep is about unchanged bytes, not about re-pinning an
# accepted construct.
#
# `refuse-inline-binary.verse` (`Add(X:int,Y:int):int = X+Y`) was removed when the
# grammar began inspecting inline binary function bodies: it no longer refuses,
# and `tests/coverage-gaps.rs` covers the accepted shape.
