# KiCad IPC API probe (K11.1)

Checks KiCad's IPC API on a nightly against KiCad 10's SWIG calls, for the KiCad 11
plugin ([K11](../../docs/roadmap/tasks/K11-kicad11-ipc-plugin.md)). The results are in
`docs/development/kicad-plugin.md`, "Nightly probe".

```sh
docker build -t freerouting-rs/kicad-nightly tools/kicad-ipc     # latest kicad-dev-nightly
python3 -m venv $V && $V/bin/pip install kicad-python grpcio-tools
# classes for KiCad's api/proto at the nightly's commit (kipy lacks the 11.0 messages)
$V/bin/python -m grpc_tools.protoc -I api/proto --python_out=$GEN <every .proto>
PY=$V/bin/python PROTO_GEN=$GEN tools/kicad-ipc/run-probe.sh WORK target/release/freerouting-rs \
  fixtures/kicad/Issue558-dev-board-autoroute-demo/dev-board.kicad_pcb ...
```

For each board, `run-probe.sh` does five things, all on copies:
- exports the DSN with KiCad 10's `ExportSpecctraDSN` and with the nightly's
  `RunBoardJobExportSpecctraDsn`, and compares the two;
- routes 10.0.7's DSN with the given router;
- imports the session with each KiCad and pairs the resulting tracks and vias
  (`../kicad-reader/frame_check.py compare`);
- repeats the import after locking 10 tracks and a via;
- checks that an unsaved edit reaches `SaveDocumentToString` and `SaveCopyOfDocument`.

| File | What |
|---|---|
| `Dockerfile` | Ubuntu 24.04 with `kicad-nightly` from the PPA; `kicad-cli-nightly api-server` runs headless |
| `probe.py` | A small NNG client for the API commands, plus `dsn-summary` and `items` (no KiCad needed) |
| `swig_side.py` | The KiCad 10 side: export, import, lock (runs under a Python that imports `pcbnew`) |
| `run-probe.sh` | The whole comparison for a list of boards |

How the probe stays clear of the KiCads in use:
- It never starts a KiCad GUI.
- It touches neither the Windows KiCad nor a running KiCad.
- It never imports the plugin package.
- The API socket is dialled by a relative path, because a Unix socket path must stay under
  108 bytes.
