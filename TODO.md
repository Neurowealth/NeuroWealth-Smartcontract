# TODO

- [ ] Add/confirm per-user strategy storage key and API:
  - [x] `DataKey::UserStrategy(Address)` exists
  - [x] `set_user_strategy(env, user, strategy)` requires `user.require_auth()`
  - [x] `get_user_strategy(env, user) -> Symbol` implemented
- [ ] Event + documentation:
  - [x] `UserStrategyUpdatedEvent { user, old_strategy, new_strategy }` exists in contract
  - [x] Document `UserStrategyUpdatedEvent` in `EVENTS.md`
  - [x] Ensure event topic constant for strategy updates is canonical (avoid using `TOPIC_USER_CAP_UPDATED`)
- [ ] Tests:
  - [x] Unit tests cover set/get roundtrip for all strategy symbols
  - [x] Tests cover auth enforcement for unauthorized updates
  - [x] Tests cover event payload correctness
  - [x] Tests cover default strategy on first deposit

