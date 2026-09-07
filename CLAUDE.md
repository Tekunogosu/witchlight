# Witchlight map service

## Comments and config notes

Write every doc comment and every note in the `NOTES` table in plain declarative
sentences. State what the thing does. For a setting, state what it controls, what
`true` does, what `false` does, the default, and which half of Witchlight reads it.
Put design reasons in a code comment beside the code they explain, and only when
the code cannot say it.

Write for someone reading the line for the first time. A sentence has a subject
and a verb and makes one claim. Nominal clauses ("Whether where...", "What the
operator...") and asides set off with dashes are not allowed.

Example of a config note:

```
# Controls who can see a new marker when its owner has not chosen.
# true: new markers are visible to everyone. false: only their owner sees them.
# Default: false. Read by both the mod and the map service.
```

## Building and installing

See the memory notes in `~/.claude/projects/-home-theysa-Development-rust-witchlight/`
for the release procedure: bump both repos together, `cargo build --release`, then
`package.sh` from the mod repo installs to all three Mods folders.
