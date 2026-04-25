# Aria Underlay

Aria Underlay is the physical switch control subsystem for Aria.

Architecture:

```text
Rust Underlay Core
    |
    | gRPC / Protobuf
    v
Python Underlay Adapter
    |
    +-- ncclient / NETCONF
    +-- NAPALM
    +-- Netmiko / SSH CLI
    |
    v
Physical Switches
```

See:

- [requirements](docs/aria-underlay-requirements.md)
- [development plan](docs/aria-underlay-development-plan.md)
- [implementation plan](docs/implementation-plan.md)

