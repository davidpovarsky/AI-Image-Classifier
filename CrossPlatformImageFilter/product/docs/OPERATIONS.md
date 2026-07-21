# Operations

Operators monitor aggregate service health, engine/capture state, policy acknowledgement, entitlement, update status, and redacted audit events. Policy and license outages use bounded exponential backoff and do not disable active filtering.

Recovery runbooks cover engine crash, capture failure, occupied port, corrupt/missing model, invalid policy, disk-full update, clock skew, and interrupted install/uninstall. Any recovery that cannot prove safe capture disables capture and restores proxy before returning degraded state.
