"""K11.1: probe KiCad's IPC API for the plugin's Specctra round trip.

    python3 probe.py --gen GEN --socket SOCK version
    python3 probe.py --gen GEN --socket SOCK export BOARD OUT.dsn
    python3 probe.py --gen GEN --socket SOCK import BOARD SESSION.ses
    python3 probe.py --gen GEN --socket SOCK save-copy BOARD OUT.kicad_pcb [--with-project]
    python3 probe.py --gen GEN --socket SOCK to-string BOARD OUT.kicad_pcb
    python3 probe.py --gen GEN --socket SOCK add-track BOARD LAYER NET X1 Y1 X2 Y2 WIDTH  (nm)
    python3 probe.py --gen GEN --socket SOCK modified BOARD
    python3 -I probe.py dsn-summary A.dsn B.dsn
    python3 -I probe.py items BOARD.kicad_pcb OUT.json

BOARD is the board's file name as KiCad has it open (`dev-board.kicad_pcb`); the
other paths are as KiCad sees them. SOCK is the API socket (`kicad-cli api-server
--socket SOCK board.kicad_pcb` on KiCad 10.99 or later).

The client talks to KiCad directly over NNG, without kipy: kicad-python 0.8.0 has
no wrapper and no generated classes for the 11.0 messages
(`RunBoardJobExportSpecctraDsn`, `ImportSpecctraSession`). Loading newer classes
next to kipy's would clash in protobuf's descriptor pool. GEN is the folder of
modules generated from KiCad's `api/proto` at the commit being probed:

    python -m grpc_tools.protoc -I api/proto --python_out=GEN <every .proto>

It needs `pynng` and `protobuf`, which `pip install kicad-python` brings along.

`dsn-summary` and `items` need no KiCad.
- `dsn-summary` counts the DSN keywords that decide routing in two files, and
  prints the lines that differ, ignoring the header lines that name the KiCad build.
- `items` writes the tracks, arcs and vias of a saved board as JSON, in the form of
  `tools/kicad-reader/frame_check.py dump`, so `frame_check.py compare` can pair
  the items of two imports.
"""

import json
import re
import sys

NM_PER_MM = 1_000_000


class ApiError(Exception):
    pass


class Client:
    def __init__(self, socket_path, gen_dir, timeout_ms=60_000):
        sys.path.insert(0, gen_dir)
        import pynng
        from common import envelope_pb2

        self.envelope = envelope_pb2
        url = socket_path if socket_path.startswith("ipc://") else "ipc://" + socket_path
        self.sock = pynng.Req0(dial=url, recv_timeout=timeout_ms, send_timeout=timeout_ms)
        self.token = ""

    def call(self, message, response_type=None):
        request = self.envelope.ApiRequest()
        request.header.kicad_token = self.token
        request.header.client_name = "freerouting-rs-probe"
        request.message.Pack(message)
        self.sock.send(request.SerializeToString())
        response = self.envelope.ApiResponse()
        response.ParseFromString(self.sock.recv())
        name = type(message).__name__
        if response.status.status != self.envelope.AS_OK:
            code = self.envelope.ApiStatusCode.Name(response.status.status)
            raise ApiError(f"{name}: {code}: {response.status.error_message}")
        self.token = response.header.kicad_token or self.token
        if response_type is None:
            return response.message.type_url
        out = response_type()
        if not response.message.Unpack(out):
            raise ApiError(f"{name}: unexpected response {response.message.type_url}")
        return out


def board_spec(board_filename):
    from common.types import base_types_pb2

    spec = base_types_pb2.DocumentSpecifier()
    spec.type = base_types_pb2.DOCTYPE_PCB
    spec.board_filename = board_filename
    return spec


def cmd_version(client):
    from common.commands import base_commands_pb2, editor_commands_pb2
    from common.types import base_types_pb2

    v = client.call(base_commands_pb2.GetVersion(), base_commands_pb2.GetVersionResponse).version
    print(f"KICAD_VERSION {v.full_version} ({v.major}.{v.minor}.{v.patch})")
    docs = client.call(
        editor_commands_pb2.GetOpenDocuments(type=base_types_pb2.DOCTYPE_PCB),
        editor_commands_pb2.GetOpenDocumentsResponse,
    )
    for d in docs.documents:
        print(f"OPEN {d.board_filename}")
    return 0


def cmd_export(client, board, out):
    from board import board_jobs_pb2
    from common.types import jobs_pb2

    job = board_jobs_pb2.RunBoardJobExportSpecctraDsn()
    job.job_settings.document.CopyFrom(board_spec(board))
    job.job_settings.output_path = out
    r = client.call(job, jobs_pb2.RunJobResponse)
    print(f"EXPORT status={jobs_pb2.JobStatus.Name(r.status)} outputs={list(r.output_path)} message={r.message!r}")
    return 0 if r.status == jobs_pb2.JS_SUCCESS else 1


def cmd_import(client, board, session):
    from board import board_commands_pb2

    req = board_commands_pb2.ImportSpecctraSession(session_path=session)
    req.board.CopyFrom(board_spec(board))
    r = client.call(req, board_commands_pb2.ImportSpecctraSessionResponse)
    print(f"IMPORT skipped={r.skipped_count}")
    print("REPORT " + r.report.replace("\n", "\nREPORT "))
    return 0


def cmd_save_copy(client, board, out, with_project=""):
    from common.commands import editor_commands_pb2

    req = editor_commands_pb2.SaveCopyOfDocument(path=out)
    req.document.CopyFrom(board_spec(board))
    req.options.overwrite = True
    req.options.include_project = with_project == "--with-project"
    print("SAVE_COPY " + client.call(req))
    return 0


def cmd_to_string(client, board, out):
    from common.commands import editor_commands_pb2

    req = editor_commands_pb2.SaveDocumentToString()
    req.document.CopyFrom(board_spec(board))
    r = client.call(req, editor_commands_pb2.SavedDocumentResponse)
    with open(out, "w", encoding="utf-8") as f:
        f.write(r.contents)
    print(f"TO_STRING {len(r.contents)} characters")
    return 0


def cmd_modified(client, board):
    from common.commands import editor_commands_pb2

    req = editor_commands_pb2.GetDocumentModifiedState()
    req.document.CopyFrom(board_spec(board))
    r = client.call(req, editor_commands_pb2.GetDocumentModifiedStateResponse)
    print("MODIFIED " + editor_commands_pb2.DocumentModifiedState.Name(r.state))
    return 0


def cmd_add_track(client, board, layer, net, x1, y1, x2, y2, width):
    """Adds one track in a commit, the way a plugin edits the open board."""
    from board import board_types_pb2
    from common.commands import editor_commands_pb2
    from common.types import base_types_pb2

    header = base_types_pb2.ItemHeader()
    header.document.CopyFrom(board_spec(board))
    commit = client.call(
        editor_commands_pb2.BeginCommit(header=header), editor_commands_pb2.BeginCommitResponse
    )
    track = board_types_pb2.Track()
    track.start.x_nm, track.start.y_nm = int(x1), int(y1)
    track.end.x_nm, track.end.y_nm = int(x2), int(y2)
    track.width.value_nm = int(width)
    track.layer = board_types_pb2.BoardLayer.Value(layer)
    track.net.name = net
    create = editor_commands_pb2.CreateItems(header=header)
    create.items.add().Pack(track)
    created = client.call(create, editor_commands_pb2.CreateItemsResponse)
    client.call(
        editor_commands_pb2.EndCommit(
            id=commit.id, action=editor_commands_pb2.CMA_COMMIT, message="K11.1 probe track"
        )
    )
    print(f"ADD_TRACK created={len(created.created_items)}")
    return 0


# --- no KiCad needed below --------------------------------------------------

DSN_KEYWORDS = (
    "layer", "class", "net", "pins", "image", "padstack", "pin", "place", "component",
    "keepout", "via_keepout", "wire_keepout", "place_keepout", "plane", "wire", "via",
    "rule", "clearance", "width", "boundary", "polygon", "path", "circle", "rect",
)
DSN_HEADER = re.compile(r"\((host_cad|host_version|parser|string_quote|space_in_quoted_tokens)\b")


def dsn_counts(text):
    counts = {}
    for m in re.finditer(r"\(([a-z_]+)", text):
        if m.group(1) in DSN_KEYWORDS:
            counts[m.group(1)] = counts.get(m.group(1), 0) + 1
    return counts


def cmd_dsn_summary(a_path, b_path):
    import difflib

    texts = [open(p, encoding="utf-8").read() for p in (a_path, b_path)]
    ca, cb = (dsn_counts(t) for t in texts)
    for k in DSN_KEYWORDS:
        if ca.get(k, 0) or cb.get(k, 0):
            mark = "" if ca.get(k, 0) == cb.get(k, 0) else "   <- differs"
            print(f"{k:14} {ca.get(k, 0):8} {cb.get(k, 0):8}{mark}")
    lines = [[l for l in t.splitlines() if not DSN_HEADER.search(l)] for t in texts]
    diff = [l for l in difflib.unified_diff(lines[0], lines[1], a_path, b_path, lineterm="", n=0)
            if not l.startswith(("---", "+++", "@@"))]
    print(f"DIFF_LINES {len(diff)}")
    for l in diff[:60]:
        print(l)
    return 0 if not diff else 1


def sexpr(text):
    """A KiCad s-expression as nested lists of strings."""
    stack, cur = [], []
    for m in re.finditer(r'\(|\)|"((?:[^"\\]|\\.)*)"|[^\s()"]+', text):
        tok = m.group(0)
        if tok == "(":
            stack.append(cur)
            cur = []
        elif tok == ")":
            done, cur = cur, stack.pop()
            cur.append(done)
        else:
            cur.append(m.group(1) if tok.startswith('"') else tok)
    return cur[0]


def field(node, key):
    for child in node[1:]:
        if isinstance(child, list) and child and child[0] == key:
            return child[1:]
    return None


def nm(value):
    return int(round(float(value) * NM_PER_MM))


def cmd_items(board_path, out_path):
    tree = sexpr(open(board_path, encoding="utf-8").read())
    nets = {n[1]: n[2] for n in tree[1:] if isinstance(n, list) and n and n[0] == "net" and len(n) > 2}
    items, locked = [], 0
    for node in tree[1:]:
        if not isinstance(node, list) or node[0] not in ("segment", "arc", "via"):
            continue
        net = (field(node, "net") or [""])[0]
        net = nets.get(net, net)
        lock = field(node, "locked")
        if lock is not None and (not lock or lock[0] == "yes") or "locked" in node[1:2]:
            locked += 1
        if node[0] == "via":
            at = [nm(v) for v in field(node, "at")]
            layers = field(node, "layers")
            items.append({"kind": "via", "layer": f"{layers[0]}-{layers[-1]}", "net": net,
                          "width": nm(field(node, "size")[0]), "start": at, "end": at})
        else:
            s = [nm(v) for v in field(node, "start")]
            e = [nm(v) for v in field(node, "end")]
            if e < s:
                s, e = e, s
            items.append({"kind": "track" if node[0] == "segment" else "arc",
                          "layer": field(node, "layer")[0], "net": net,
                          "width": nm(field(node, "width")[0]), "start": s, "end": e})
    items.sort(key=lambda i: (i["kind"], i["layer"], i["net"], i["width"], i["start"], i["end"]))
    kinds = {k: sum(1 for i in items if i["kind"] == k) for k in ("track", "arc", "via")}
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump({"source": board_path, "items": items, "locked": locked, "counts": kinds},
                  f, indent=1, sort_keys=True)
    print(f"ITEMS tracks={kinds['track']} arcs={kinds['arc']} vias={kinds['via']} locked={locked}")
    return 0


def main(argv):
    if argv and argv[0] == "dsn-summary":
        return cmd_dsn_summary(*argv[1:3])
    if argv and argv[0] == "items":
        return cmd_items(*argv[1:3])
    if len(argv) < 5 or argv[0] != "--gen" or argv[2] != "--socket":
        print(__doc__)
        return 2
    client = Client(argv[3], argv[1])
    command, args = argv[4], argv[5:]
    commands = {
        "version": cmd_version, "export": cmd_export, "import": cmd_import,
        "save-copy": cmd_save_copy, "to-string": cmd_to_string, "modified": cmd_modified,
        "add-track": cmd_add_track,
    }
    try:
        return commands[command](client, *args)
    except ApiError as e:
        print(f"API_ERROR {e}")
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
