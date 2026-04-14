# adroit

A snappy TUI for managing Architecture Decision Records.

The name hides **ADR** in plain sight — because good architecture decisions should be _adroit_: clever, skillful, and well-considered.

## What are ADRs?

Architecture Decision Records are short documents that capture important architectural decisions along with their context and consequences. They provide a decision log that helps current and future team members understand _why_ the system looks the way it does.

## What does adroit do?

adroit gives you a terminal-native interface for the full ADR lifecycle:

- **Create** new ADRs from templates with guided prompts
- **Browse** and search your decision log in a rich, interactive TUI
- **Update** status as decisions are superseded, deprecated, or accepted
- **Link** related decisions together to build a navigable decision graph
- **Export** to Markdown for integration with your existing docs pipeline

## Installation

```sh
cargo install adroit
```

## Usage

```sh
# Initialize an ADR directory in your project
adroit init

# Create a new ADR
adroit new "Use PostgreSQL for primary datastore"

# Launch the interactive TUI
adroit
```

## License

Apache-2.0
