# Windows service

Install the supervisor as a per-machine Windows Service under a dedicated restricted service SID with automatic delayed start and recovery actions. Named-pipe ACLs admit only the service, the signed desktop client identity, and administrators; the server additionally validates client PID/token and executable signature.

Proxy state uses exact per-user/system snapshots appropriate to the selected capture mode. DPAPI protects device and CA private material. The service must not enable capture until engine, policy, models, CA, and loopback health pass.
