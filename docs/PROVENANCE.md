# Provenance ledger

One row per port of third-party (non-Java) code. Ported files carry a
`Portions ported from` line and this SPDX header:

    SPDX-License-Identifier: GPL-3.0-only AND MIT
    SPDX-FileCopyrightText: 2026 drandyhaas (KiCadRoutingTools)

`tools/check-provenance.sh` checks the ledger against the tree. Table rows
below are parsed: first column is the Rust file path (backticked).

| Rust file | Symbol | Source path | Source symbol | Source commit | Date | Kind |
|---|---|---|---|---|---|---|
