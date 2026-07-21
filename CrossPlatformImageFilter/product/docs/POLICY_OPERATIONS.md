# Policy operations

Download into a product-owned staging directory, verify TUF metadata and target hash/size, verify the bundle signature/schema/subject/revision/version, dry-construct the engine configuration, preserve the active bundle as last-known-good, atomically rename, reload, and health-check. Activation failure restores the previous policy and never disables filtering because the server is unreachable.

Refresh uses jittered scheduling with exponential backoff. Manual “check” performs the identical verification path. Control-plane publication writes targets and all TUF metadata in one transaction and rolls back incomplete uploads.
