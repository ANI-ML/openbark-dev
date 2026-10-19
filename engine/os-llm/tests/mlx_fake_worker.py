"""A stand-in for OpenBark's MLX worker, for tests (standard library only).

It speaks the worker protocol (docs/plans/mlx-models.md, contract 9) and
behaves according to the loaded model folder's name:

  ok              streams a reply; under a schema, answers {"ok":true} or a small object
  unconstrained   reports constrained_ready false
  badcheck        under a schema, answers text that isn't JSON
  slow            streams many tokens slowly, and honours cancel
  slowprompt      reports prompt progress for about 3 s before the first token
  deaf            streams many tokens and ignores cancel
  noisy           prints to stdout (redirected), splits lines, sends a garbage line and a 1 MB token
  envcheck        answers with its environment, flags and folder as JSON
  fail-<kind>     generate fails with that error kind
  fail-load       load fails (kind load)
  fail-arch       load fails (kind unsupported_architecture)
  crash-once      the first generate crashes mid-stream; later ones work
  crash-always    every generate crashes mid-stream
  hang-once       the first generate goes silent; later ones work
  stubborn        ignores shutdown and stdin closing (Drop must kill it)

It counts loads and generates in files in the model folder, and leaves a
"shutdown" file there when it is asked to stop.
"""

import json
import os
import sys
import threading
import time

# Like the real worker: the protocol stream is a copy of fd 1, and fd 1 goes
# to stderr, so a stray print can't corrupt the protocol.
PROTO = os.fdopen(os.dup(1), "w", encoding="utf-8", newline="\n", buffering=1)
os.dup2(2, 1)
sys.stdout = sys.stderr

LOCK = threading.Lock()
CANCEL = threading.Event()
STATE = {"model": None, "name": "", "running": None, "ctx": 40960}
REQUESTS = []
COND = threading.Condition()


def send(obj):
    with LOCK:
        PROTO.write(json.dumps(obj, ensure_ascii=False) + "\n")
        PROTO.flush()


def bump(name):
    folder = STATE["model"]
    if not folder:
        return 0
    path = os.path.join(folder, name)
    try:
        with open(path) as f:
            n = int(f.read().strip() or "0")
    except OSError:
        n = 0
    n += 1
    with open(path, "w") as f:
        f.write(str(n))
    return n


def once(marker):
    """True the first time for this model folder."""
    path = os.path.join(STATE["model"], marker)
    if os.path.exists(path):
        return False
    with open(path, "w") as f:
        f.write("1")
    return True


def reader():
    while True:
        line = sys.stdin.readline()
        if not line:
            break
        line = line.strip()
        if not line:
            continue
        req = json.loads(line)
        if req.get("op") == "cancel":
            if STATE["running"] is not None and req.get("id") == STATE["running"]:
                CANCEL.set()
            continue
        with COND:
            REQUESTS.append(req)
            COND.notify()
    with COND:
        REQUESTS.append({"op": "__eof__"})
        COND.notify()


def next_request():
    with COND:
        while not REQUESTS:
            COND.wait()
        return REQUESTS.pop(0)


def error(req_id, kind, message="the fake worker was told to fail", **numbers):
    out = {"id": req_id, "event": "error", "kind": kind, "message": message}
    out.update(numbers)
    send(out)


def stats(prompt, completion):
    return {"prompt_tokens": prompt, "completion_tokens": completion, "time_to_first_token_ms": 5,
            "total_ms": 20, "tokens_per_second": 12.5, "hit_max_tokens": False, "peak_memory_bytes": 1234}


def stream(req_id, pieces, delay=0.0, honour_cancel=True):
    sent = []
    for p in pieces:
        if honour_cancel and CANCEL.is_set():
            return sent, "cancelled"
        send({"id": req_id, "event": "token", "text": p})
        sent.append(p)
        if delay:
            time.sleep(delay)
    return sent, "stop"


def generate(req):
    req_id = req["id"]
    name = STATE["name"]
    bump("generates")
    schema = req.get("json_schema")
    prompt_tokens = sum(len(m["content"]) for m in req["messages"])
    # Like the real worker: the prompt (here, its characters) plus the new
    # tokens must fit min(the model's context, max_context).
    if prompt_tokens + req["max_tokens"] > STATE["ctx"]:
        return error(req_id, "context_overflow", "the prompt doesn't fit the model's context",
                     prompt_tokens=prompt_tokens, max_new=req["max_tokens"], context=STATE["ctx"])
    if name.startswith("fail-"):
        kind = name[len("fail-"):]
        if kind == "context_overflow":
            return error(req_id, kind, "the prompt doesn't fit the model's context",
                         prompt_tokens=5000, max_new=req["max_tokens"], context=4096)
        return error(req_id, kind)
    if name == "crash-always" or (name == "crash-once" and once("crashed")):
        send({"id": req_id, "event": "token", "text": "partial"})
        time.sleep(0.05)
        os._exit(1)
    if name == "hang-once" and once("hung"):
        send({"id": req_id, "event": "token", "text": "then silence"})
        time.sleep(3600)
    if name == "envcheck":
        body = json.dumps({
            "env": dict(os.environ),
            "isolated": sys.flags.isolated,
            "dont_write_bytecode": sys.flags.dont_write_bytecode,
            "utf8_mode": sys.flags.utf8_mode,
            "cwd": os.getcwd(),
            "script": os.path.abspath(__file__),
        })
        sent, finish = stream(req_id, [body])
    elif schema is not None:
        if name == "badcheck":
            pieces = ["not ", "json"]
        elif '"ok"' in schema:
            pieces = ['{"ok"', ":true}"]
        else:
            pieces = ['{"note":', '"fine"}']
        sent, finish = stream(req_id, pieces)
    elif name == "slowprompt":
        for done in range(0, 7):
            send({"id": req_id, "event": "progress", "prompt_done": done * 64, "prompt_total": 400})
            time.sleep(0.4)
        sent, finish = stream(req_id, ["read ", "it all"])
    elif name == "slow":
        sent, finish = stream(req_id, ["t%d " % i for i in range(200)], delay=0.05)
    elif name == "deaf":
        sent, finish = stream(req_id, ["d%d " % i for i in range(30)], delay=0.01, honour_cancel=False)
    elif name == "noisy":
        print("library noise on stdout")
        sys.stderr.write("library noise on stderr\n")
        with LOCK:
            PROTO.write("this line isn't JSON\n")
            PROTO.write("\n")
            half = json.dumps({"id": req_id, "event": "token", "text": "split "})
            PROTO.write(half[:7])
            PROTO.flush()
            time.sleep(0.05)
            PROTO.write(half[7:] + "\n")
            PROTO.write(json.dumps({"id": 999, "event": "token", "text": "someone else's"}) + "\n")
            PROTO.flush()
        big = "x" * (1 << 20)
        sent, finish = stream(req_id, [big, "é\U0001F415\n\"q\""])
        sent = ["split "] + sent
    else:
        sent, finish = stream(req_id, ["Hello", ", ", "wörld", " \U0001F43E", "\nbye"])
    send({"id": req_id, "event": "done", "text": "".join(sent), "finish": finish,
          "stats": stats(prompt_tokens, len(sent))})


def main():
    if os.environ.get("HF_HUB_OFFLINE") != "1":
        sys.stderr.write("fake worker: refusing to start without HF_HUB_OFFLINE=1\n")
        sys.exit(3)
    threading.Thread(target=reader, daemon=True).start()
    while True:
        req = next_request()
        op = req.get("op")
        if op in ("shutdown", "__eof__"):
            if STATE["model"]:
                with open(os.path.join(STATE["model"], "shutdown"), "w") as f:
                    f.write(op)
            if STATE["name"] == "stubborn":
                time.sleep(3600)
            break
        CANCEL.clear()
        if op == "hello":
            send({"event": "hello", "protocol": 1, "python": "%d.%d.%d" % sys.version_info[:3],
                  "versions": {"mlx": "fake", "mlx_lm": "fake", "transformers": "fake", "llguidance": "fake"},
                  "device": "cpu", "metal": False})
        elif op == "load":
            path = req.get("path", "")
            if not os.path.isabs(path) or not os.path.isdir(path):
                error(req.get("id"), "bad_request", "load needs the full path of a model folder")
                continue
            STATE["model"] = path
            STATE["name"] = os.path.basename(os.path.normpath(path))
            cap = req.get("max_context") or 0
            STATE["ctx"] = min(40960, cap) if cap > 0 else 40960
            bump("loads")
            if STATE["name"] == "fail-load":
                error(req["id"], "load", "the model couldn't be loaded")
                continue
            if STATE["name"] == "fail-arch":
                error(req["id"], "unsupported_architecture", "the MLX engine can't run this kind of model yet (fake)")
                continue
            send({"id": req["id"], "event": "loaded", "model_type": "qwen3", "context_length": STATE["ctx"],
                  "eos_token_ids": [151645, 151643], "vocab_size": 151936,
                  "constrained_ready": STATE["name"] != "unconstrained", "load_ms": 3})
        elif op == "generate":
            STATE["running"] = req["id"]
            try:
                generate(req)
            finally:
                STATE["running"] = None
        elif op == "selftest":
            send({"id": req["id"], "event": "selftest", "ok": True,
                  "checks": [["imports", True], ["socket_guard", True], ["metal", True], ["matmul", True],
                             ["mask", True]], "message": None})
        elif op == "unload":
            send({"id": req["id"], "event": "unloaded"})
        else:
            error(req.get("id"), "bad_request", "unknown request")
    os._exit(0)


if __name__ == "__main__":
    main()
