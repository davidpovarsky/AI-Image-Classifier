# Linux systemd

The deb/rpm packages install a dedicated system user, root-owned application directories, a runtime socket directory, and a hardened unit with `NoNewPrivileges`, private temporary storage, restrictive filesystem access, capability minimization, and restart limits.

Secret Service is preferred for user-bound secrets; root-owned encrypted storage is the documented fallback. Package removal runs exact proxy/CA recovery before deleting the unit and product-owned files. Desktop proxy differences require manual qualification.
