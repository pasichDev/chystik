# Benchmarks

Chystik's benchmark answers one narrow question: how long does **scanning** a
known tree take? It does not measure cleanup, native Trash, GUI rendering or a
developer's entire disk. Do not treat a reference number as a product
guarantee.

## Reproducible synthetic benchmark

The fixture is deliberately independent of `$HOME` and never runs cleanup. It
contains nested directories, exact Chystik fixtures (pip, `node_modules` with
a lockfile, Cargo `target`), irrelevant siblings, different file sizes and
protected-looking `.git`/`.ssh` paths. The latter are input only: nothing in
this workflow deletes or modifies a path after fixture creation.

```bash
fixture="$(mktemp -d)/chystik-benchmark"
bash scripts/benchmark-fixture.sh "$fixture"
bash scripts/benchmark-scan.sh "$fixture" --runs 10
```

When installed, `hyperfine` provides warm-up, median and min/max statistics.
Without it, the script falls back to the shell's `time` command for each run.
Neither tool is a Chystik build dependency. Run the command once after a cold
boot or cache drop only if your operating system's policy permits it, then run
again with a warm filesystem cache; report those states separately.

Record all of the following with a result:

| Field | What to record |
| --- | --- |
| Chystik | workspace version and commit |
| Hardware | CPU model, core count, RAM and storage type |
| System | OS, kernel, architecture and Rust version |
| Filesystem | filesystem type and mount options for the target |
| Target | fixture/real path, entry count and approximate bytes |
| Cache state | cold or warm, plus how that was established |
| Command | exact command including run count |
| Result | median and min/max (or percentile) |

Count fixture entries and bytes without cleanup:

```bash
find "$fixture" -xdev -printf . | wc -c
du -sh "$fixture"
```

## Reference measurement, not a guarantee

The maintainer updates this row only together with the command and environment
evidence. It is useful as a regression reference, not a claim about scans of
`/` or another user's machine.

| Captured | Chystik | Environment | Target | Cache state | Command | Runs | Result |
| --- | --- | --- | --- | --- | --- | ---: | --- |
| 2026-08-26 | `0.2.1`, local workspace based on `de5a703` | AMD Ryzen 5 3600 (12 logical CPUs), 31 GiB RAM; Linux 7.0.0-30-generic x86_64; Rust/Cargo 1.98.0; fixture on tmpfs | 427 entries, 1.6 MiB | warm; release binary built before timing | `bash scripts/benchmark-scan.sh "$fixture" --runs 10` | 10 | median 7 ms; min 6 ms; max 7 ms |

## Real-world reference benchmark

For a real machine, scan an explicit, non-root directory and include the same
metadata. Do not publish private path names, user names, repository names, or
file listings. A real-world result should be labelled **Reference measurement,
not a guarantee**, and kept distinct from the synthetic fixture result.

Normal CI only checks that fixture creation and one scan execute successfully.
It intentionally does not set a wall-clock performance gate on shared runners.
