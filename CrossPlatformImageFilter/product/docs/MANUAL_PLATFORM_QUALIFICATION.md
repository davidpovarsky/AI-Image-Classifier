# Manual platform qualification

GitHub-hosted CI cannot prove reboot persistence, OS trust prompts, real browser interception, physical network recovery, code-signing reputation, notarization UX, enterprise proxy coexistence, secure-store behavior under multiple users, or forced administrator removal.

Before release, qualify clean and upgraded Windows x64, macOS Apple Silicon/Intel, Ubuntu and rpm-based Linux machines. Test install, consent, first start, browser traffic, pinned-client behavior, engine/service crash, reboot, sleep/network change, policy/license outage, update/rollback, password/recovery, protected uninstall, exact proxy restoration, exact CA removal, and absence of a remaining service or broken route.
