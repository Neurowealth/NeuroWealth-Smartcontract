# Runtime safety integration notes

The bridge and agent runtime changes in the assigned issues require the
application packages that own bridge recovery, bridge persistence, schema
generation, and failover bootstrapping. Those packages are not present in this
repository checkout, so their safety requirements are recorded here rather
than represented as an unverified code change.

- A reconciliation dry run must never call an external transfer method.
- Database persistence must map application fields to declared database column
  names and serialise bigint/timestamp values safely.
- Schema-drift checks must watch migration sources and verify generated types.
- Agent loops must be gated by a leased primary role before they perform work.
