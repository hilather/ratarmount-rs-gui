# Implementation

How this repository is built in waves of subagents. Engine work (G0–G7) lives in **ratarmount-rs** and is an external gate, not a PR here.

| Doc | Topic |
|-----|--------|
| [plan.md](plan.md) | **First-class** orchestrator plan: wave table, spawn prompts, fake-session strategy, `/execute-plan` mapping |
| [06-agent-waves.md](06-agent-waves.md) | Wave index + ownership + agent rules |
| [waves/W0.md](waves/W0.md) … [W8.md](waves/W8.md) | Per-wave checklists (unchecked until the wave lands) |

Architecture must be read before code: [../architecture/](../architecture/). Acceptance: [../design/07-acceptance.md](../design/07-acceptance.md).
