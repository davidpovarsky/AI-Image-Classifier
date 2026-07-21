# Linux packaging

Build native deb and rpm packages containing `/opt/local-ai-image-filter`, the hardened unit, checksums, and repository-signing hooks. Maintainer scripts call the supervisor recovery path before stopping the service. An AppImage is not a production substitute because the product needs a system service.

Repository metadata and package signatures are generated only with protected release keys. Internal CI packages remain unsigned and clearly labeled.
