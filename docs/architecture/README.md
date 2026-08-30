# Architecture

Current-state design for `ratarmount-rs-gui`. Update these files in the same change as the code they describe. Diagrams: prefer Mermaid.

| Doc | Topic |
|-----|--------|
| [01-architecture.md](01-architecture.md) | Process shape, responsibility split, threading, session lifecycle |
| [02-index-storage.md](02-index-storage.md) | SQLite 0.7.x sidecars, policies, cache paths, config.toml |
| [03-distribution.md](03-distribution.md) | In-process crates + bundled/Depends CLI, installers |
| [04-os-integration.md](04-os-integration.md) | argv, MIME, desktop/plist/registry |
| [05-napi-contract.md](05-napi-contract.md) | **Only** API React may call |

Load-bearing decisions: [../adr/](../adr/). Consolidated design: [../design/design.md](../design/design.md).
