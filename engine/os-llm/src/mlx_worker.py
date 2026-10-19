"""OpenBark's MLX worker (ADR-0025 decision 2).

Runs inside the MLX engine pack's own Python, started by OpenBark as
`python3 -I -B -X utf8 -u openbark_mlx_worker.py`. It talks only over its
standard input and output, one JSON object per line (the protocol is in
docs/plans/mlx-models.md, contract 9). Its own code opens no socket, binds no
port and never uses the network. Prompts and generated text go only to the
protocol stream: never to stderr, a file or a log.

Network protection, in layers (mlx.rs starts it): an empty environment with
the Hugging Face libraries offline, a local folder only, and on macOS an OS
sandbox denying all network access. The guard below is a best-effort extra
for Python code: it doesn't reach the low-level _socket module or native
code in the pack's libraries.

Written into the pack folder by OpenBark (`mlx.rs`); don't edit the copy there.
"""

# The socket guard comes first, before any other import, so Python code
# imported later (transformers, huggingface_hub, mlx-lm) can't connect, send
# to an address, bind or listen through the socket module.
import socket as _socket


class NetworkBlocked(OSError):
    """Raised for every attempt to reach the network."""


def _blocked(*_args, **_kwargs):
    raise NetworkBlocked("the OpenBark MLX worker never uses the network")


_socket.socket.connect = _blocked
_socket.socket.connect_ex = _blocked
_socket.socket.sendto = _blocked
_socket.socket.sendmsg = _blocked
_socket.socket.bind = _blocked
_socket.socket.listen = _blocked
_socket.create_connection = _blocked
_socket.getaddrinfo = _blocked

import os  # noqa: E402
import sys  # noqa: E402

if os.environ.get("HF_HUB_OFFLINE") != "1":
    sys.stderr.write("openbark mlx worker: refusing to start without HF_HUB_OFFLINE=1\n")
    sys.exit(3)

# The protocol stream is a copy of fd 1 made before fd 1 is pointed at stderr,
# so anything a library prints can't corrupt the protocol.
_PROTO_FD = os.dup(1)
os.dup2(2, 1)
sys.stdout = sys.stderr
_PROTO = os.fdopen(_PROTO_FD, "w", encoding="utf-8", errors="replace", newline="\n", buffering=1)

import gc  # noqa: E402
import json  # noqa: E402
import math  # noqa: E402
import queue  # noqa: E402
import threading  # noqa: E402
import time  # noqa: E402
from importlib import metadata as _metadata  # noqa: E402

PROTOCOL = 1
_WRITE_LOCK = threading.Lock()


def send(obj):
    line = json.dumps(obj, ensure_ascii=False, separators=(",", ":"))
    with _WRITE_LOCK:
        _PROTO.write(line + "\n")
        _PROTO.flush()


def log(msg):
    """Metadata only: timings, sizes, versions, error kinds. Never content."""
    sys.stderr.write("openbark mlx worker: %s\n" % msg)
    sys.stderr.flush()


class WorkerError(Exception):
    def __init__(self, kind, message, **numbers):
        super().__init__(message)
        self.kind = kind
        self.message = message
        self.numbers = numbers


class _Cancelled(Exception):
    """Raised from the prompt-progress callback to stop reading the prompt."""


def send_error(req_id, err):
    out = {"id": req_id, "event": "error", "kind": err.kind, "message": err.message}
    if err.kind == "context_overflow":
        out["prompt_tokens"] = int(err.numbers.get("prompt_tokens", 0))
        out["max_new"] = int(err.numbers.get("max_new", 0))
        out["context"] = int(err.numbers.get("context", 0))
    send(out)


# --------------------------------------------------------------- stdin reader

_REQUESTS = queue.Queue()
_CANCEL = threading.Event()
_STATE_LOCK = threading.Lock()
_RUNNING = {"id": None}


def _reader():
    """Reads requests; a cancel for the running generate sets the flag at once."""
    stream = sys.stdin.buffer
    while True:
        raw = stream.readline()
        if not raw:
            break
        raw = raw.strip()
        if not raw:
            continue
        try:
            req = json.loads(raw.decode("utf-8"))
            if not isinstance(req, dict):
                raise ValueError("not an object")
        except Exception:
            _REQUESTS.put({"op": "__bad__"})
            continue
        if req.get("op") == "cancel":
            with _STATE_LOCK:
                running = _RUNNING["id"]
            if running is not None and (req.get("id") is None or req.get("id") == running):
                _CANCEL.set()
            continue
        if req.get("op") == "shutdown":
            _CANCEL.set()
        _REQUESTS.put(req)
    # stdin closed: OpenBark has gone. Stop what's running and exit.
    _CANCEL.set()
    _REQUESTS.put({"op": "shutdown"})


# --------------------------------------------------------------- environment

def _version(dist):
    try:
        return _metadata.version(dist)
    except Exception:
        return None


def _versions():
    out = {}
    for key, dist in (("mlx", "mlx"), ("mlx_lm", "mlx-lm"), ("transformers", "transformers"),
                      ("llguidance", "llguidance"), ("tokenizers", "tokenizers")):
        v = _version(dist)
        if v:
            out[key] = v
    return out


_MX = None
_DEVICE = {"name": "cpu", "metal": False}


def _mlx():
    """Imports mlx.core once and picks the device: Metal unless forced to CPU."""
    global _MX
    if _MX is None:
        import mlx.core as mx

        metal = False
        try:
            metal = bool(mx.metal.is_available())
        except Exception:
            metal = False
        forced_cpu = os.environ.get("OPENBARK_MLX_DEVICE", "").strip().lower() == "cpu"
        if metal and not forced_cpu:
            mx.set_default_device(mx.gpu)
            _DEVICE["name"] = "gpu"
        else:
            mx.set_default_device(mx.cpu)
            _DEVICE["name"] = "cpu"
        _DEVICE["metal"] = metal
        _MX = mx
    return _MX


def _on_gpu():
    return _DEVICE["name"] == "gpu"


# --------------------------------------------------------------- the schema mask

def apply_mask(mx, logits, mask_words):
    """Logits where the bitmask allows a token, minus infinity elsewhere.

    `mask_words` is llguidance's int32 bitmask, shape (1, ceil(V/32)). Tokens
    past the mask (a model vocabulary larger than the tokenizer's) are masked.
    """
    if _on_gpu():
        import llguidance.mlx as llmx

        return llmx.apply_token_bitmask(logits, mask_words)
    words = mx.array(mask_words)
    shifts = mx.arange(32, dtype=mx.int32)
    allowed = ((words[:, :, None] >> shifts) & 1).reshape(words.shape[0], -1)
    vocab = logits.shape[-1]
    if allowed.shape[-1] >= vocab:
        allowed = allowed[:, :vocab]
    else:
        pad = mx.zeros((allowed.shape[0], vocab - allowed.shape[-1]), dtype=allowed.dtype)
        allowed = mx.concatenate([allowed, pad], axis=1)
    if logits.ndim == 1:
        allowed = allowed[0]
    return mx.where(allowed.astype(mx.bool_), logits, mx.array(-math.inf, logits.dtype))


class SchemaProcessor:
    """An mlx-lm logits processor holding the output to a JSON Schema."""

    def __init__(self, ll_tokenizer, grammar, prompt_len):
        import llguidance
        import llguidance.numpy as llnp

        self.llnp = llnp
        self.matcher = llguidance.LLMatcher(ll_tokenizer, grammar, log_level=0)
        if self.matcher.is_error():
            raise WorkerError("grammar", "the note's format couldn't be compiled for this model")
        self.mask = llnp.allocate_token_bitmask(1, ll_tokenizer.vocab_size)
        self.seen = prompt_len  # mlx-lm passes the prompt and every token since

    def __call__(self, tokens, logits):
        mx = _mlx()
        new = tokens[self.seen:].tolist() if tokens.size > self.seen else []
        self.seen = int(tokens.size)
        for t in new:
            if not self.matcher.consume_token(int(t)):
                # get_error() can quote generated text: never pass it on.
                raise WorkerError("inference", "the model left the note's format")
        self.llnp.fill_next_token_bitmask(self.matcher, self.mask, 0)
        return apply_mask(mx, logits, self.mask)


# --------------------------------------------------------------- the model

class Loaded:
    def __init__(self):
        self.model = None
        self.tokenizer = None
        self.ll_tokenizer = None
        self.config = {}
        # min(the model's own context, OpenBark's max_context): prompt plus
        # new tokens past this are refused, as llama.cpp refuses them.
        self.context_length = 0
        self.vocab_size = 0
        self.eos = []


_MODEL = {"loaded": None}


def _read_config(folder):
    try:
        with open(os.path.join(folder, "config.json"), "r", encoding="utf-8") as f:
            cfg = json.load(f)
    except Exception:
        raise WorkerError("load", "the model's config.json can't be read")
    if not isinstance(cfg, dict) or not isinstance(cfg.get("model_type"), str):
        raise WorkerError("load", "the model's config.json has no model_type")
    return cfg


def _architecture_supported(cfg):
    import importlib.util

    from mlx_lm.utils import MODEL_REMAPPING

    model_type = cfg["model_type"]
    names = {MODEL_REMAPPING.get(model_type, model_type)}
    try:
        from mlx_lm.utils import MODEL_ARCHITECTURE_REMAPPING

        archs = cfg.get("architectures") or ()
        if isinstance(archs, str):
            archs = (archs,)
        for a in archs:
            mapped = MODEL_ARCHITECTURE_REMAPPING.get((model_type, a))
            if mapped:
                names.add(mapped)
    except ImportError:
        pass
    for name in names:
        if not name.replace("_", "").isalnum():
            continue
        if importlib.util.find_spec("mlx_lm.models." + name) is not None:
            return True
    return False


def _config_int(cfg, keys):
    for scope in (cfg, cfg.get("text_config") or {}):
        if not isinstance(scope, dict):
            continue
        for k in keys:
            v = scope.get(k)
            if isinstance(v, int) and not isinstance(v, bool) and v > 0:
                return v
    return 0


def _eos_ids(tw, cfg):
    ids = set(int(i) for i in (tw.eos_token_ids or []))
    for src in (cfg.get("eos_token_id"), (cfg.get("text_config") or {}).get("eos_token_id")):
        if isinstance(src, int):
            ids.add(src)
        elif isinstance(src, list):
            ids.update(int(i) for i in src if isinstance(i, int))
    return sorted(ids)


def _plain(exc, folder):
    """A short, plain reason, without the folder's path."""
    text = str(exc).replace(folder, "<model folder>").strip().splitlines()
    first = text[0] if text else ""
    first = first[:200]
    return "%s%s" % (type(exc).__name__, (": " + first) if first else "")


def op_load(req):
    path = req.get("path")
    if not isinstance(path, str) or not path or not os.path.isabs(path):
        raise WorkerError("bad_request", "load needs the full path of a model folder")
    max_context = req.get("max_context", 0)
    if max_context is None:
        max_context = 0
    if isinstance(max_context, bool) or not isinstance(max_context, int) or max_context < 0:
        raise WorkerError("bad_request", "max_context must be a whole number")
    if not os.path.isdir(path):
        raise WorkerError("load", "the model folder isn't there")
    if not os.path.isfile(os.path.join(path, "config.json")):
        raise WorkerError("load", "the model folder has no config.json")
    started = time.time()
    cfg = _read_config(path)
    mx = _mlx()
    try:
        supported = _architecture_supported(cfg)
    except Exception as e:
        raise WorkerError("load", "the MLX engine's packages didn't load (%s)" % type(e).__name__)
    if not supported:
        kind = cfg["model_type"][:60]
        raise WorkerError("unsupported_architecture", "the MLX engine can't run this kind of model yet (%s)" % kind)
    op_unload_quietly()
    try:
        from mlx_lm import load as mlx_load

        model, tw, config = mlx_load(
            path,
            tokenizer_config={"trust_remote_code": False},
            return_config=True,
            trust_remote_code=False,
        )
    except WorkerError:
        raise
    except MemoryError:
        raise WorkerError("load", "there isn't enough memory for this model")
    except Exception as e:
        raise WorkerError("load", "the model couldn't be loaded: %s" % _plain(e, path))
    if not _on_gpu():
        # Quantised bf16 matmul is about 10 times slower on the CPU.
        model.set_dtype(mx.float32)
    loaded = Loaded()
    loaded.model = model
    loaded.tokenizer = tw
    loaded.config = config if isinstance(config, dict) else cfg
    model_context = _config_int(
        loaded.config, ("max_position_embeddings", "max_seq_len", "seq_length", "n_positions")) or 8192
    # The cap keeps the KV cache (and the Mac's memory) bounded, like the
    # built-in engine's max_context: many models claim 128k or more.
    loaded.context_length = min(model_context, max_context) if max_context > 0 else model_context
    loaded.vocab_size = _config_int(loaded.config, ("vocab_size",)) or len(tw._tokenizer)
    loaded.eos = _eos_ids(tw, loaded.config)
    if loaded.eos:
        tw.eos_token_ids = loaded.eos
    constrained_ready = False
    try:
        import llguidance
        import llguidance.hf
        import transformers

        hf = tw._tokenizer
        if isinstance(hf, transformers.PreTrainedTokenizerFast):
            llt = llguidance.hf.from_tokenizer(hf, eos_token=loaded.eos or None)
            probe = llguidance.LLMatcher.grammar_from_json_schema(
                {"type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"]},
                defaults={"whitespace_flexible": False})
            if not llguidance.LLMatcher(llt, probe, log_level=0).is_error():
                loaded.ll_tokenizer = llt
                constrained_ready = True
        else:
            log("no fast tokenizer for this model: JSON Schema output unavailable")
    except Exception as e:
        log("llguidance unavailable for this model (%s)" % type(e).__name__)
    _MODEL["loaded"] = loaded
    load_ms = int((time.time() - started) * 1000)
    log("loaded model_type=%s ctx=%d (model %d) vocab=%d eos=%d constrained=%s device=%s in %d ms" % (
        cfg["model_type"][:60], loaded.context_length, model_context, loaded.vocab_size, len(loaded.eos),
        constrained_ready, _DEVICE["name"], load_ms))
    send({
        "id": req.get("id"),
        "event": "loaded",
        "model_type": cfg["model_type"],
        "context_length": loaded.context_length,
        "eos_token_ids": loaded.eos,
        "vocab_size": loaded.vocab_size,
        "constrained_ready": constrained_ready,
        "load_ms": load_ms,
    })


def op_unload_quietly():
    if _MODEL["loaded"] is not None:
        _MODEL["loaded"] = None
        gc.collect()
        try:
            _mlx().clear_cache()
        except Exception:
            pass


def _messages(req):
    msgs = req.get("messages")
    if not isinstance(msgs, list) or not msgs:
        raise WorkerError("bad_request", "generate needs messages")
    out = []
    for m in msgs:
        if not isinstance(m, dict) or not isinstance(m.get("role"), str) or not isinstance(m.get("content"), str):
            raise WorkerError("bad_request", "each message needs a role and content")
        out.append({"role": m["role"], "content": m["content"]})
    return out


def _number(req, key, default, cast, lo, hi):
    v = req.get(key, default)
    if v is None:
        v = default
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        raise WorkerError("bad_request", "%s must be a number" % key)
    v = cast(v)
    if not (lo <= v <= hi):
        raise WorkerError("bad_request", "%s is out of range" % key)
    return v


def op_generate(req):
    req_id = req.get("id")
    loaded = _MODEL["loaded"]
    if loaded is None:
        raise WorkerError("bad_request", "no model is loaded")
    messages = _messages(req)
    max_tokens = _number(req, "max_tokens", 2048, int, 1, 1 << 20)
    temperature = _number(req, "temperature", 0.2, float, 0.0, 5.0)
    seed = _number(req, "seed", 42, int, 0, (1 << 32) - 1)
    schema = req.get("json_schema")
    if schema is not None and not isinstance(schema, str):
        raise WorkerError("bad_request", "json_schema must be a string")

    mx = _mlx()
    tw = loaded.tokenizer
    try:
        prompt = tw.apply_chat_template(messages, add_generation_prompt=True, enable_thinking=False)
    except Exception as e:
        raise WorkerError("template", "the model's chat template failed (%s)" % type(e).__name__)
    if isinstance(prompt, str):
        prompt = tw._tokenizer.encode(prompt, add_special_tokens=False)
    if isinstance(prompt, dict) or hasattr(prompt, "keys"):
        prompt = prompt["input_ids"]
    prompt = [int(t) for t in prompt]
    if len(prompt) + max_tokens > loaded.context_length:
        raise WorkerError("context_overflow", "the prompt doesn't fit the model's context",
                          prompt_tokens=len(prompt), max_new=max_tokens, context=loaded.context_length)

    processors = []
    if schema is not None:
        if loaded.ll_tokenizer is None:
            raise WorkerError("grammar", "this model can't be held to the note's format on MLX")
        try:
            import llguidance

            parsed = json.loads(schema)
            if not isinstance(parsed, dict):
                raise ValueError("not an object")
            grammar = llguidance.LLMatcher.grammar_from_json_schema(
                parsed, defaults={"whitespace_flexible": False})
        except Exception:
            raise WorkerError("grammar", "the note's format couldn't be compiled for this model")
        processors.append(SchemaProcessor(loaded.ll_tokenizer, grammar, len(prompt)))

    from mlx_lm import stream_generate
    from mlx_lm.sample_utils import make_sampler

    mx.random.seed(seed)
    try:
        mx.reset_peak_memory()
    except Exception:
        pass
    # The same selection as the built-in llama.cpp engine: greedy at
    # temperature 0, otherwise top-k 40, top-p 0.95, then temperature.
    if temperature <= 0.0:
        sampler = make_sampler(temp=0.0)
    else:
        sampler = make_sampler(temp=temperature, top_p=0.95, top_k=40)
    parts = []
    last = None
    finish = None
    started = time.time()
    first_ms = None
    try:
        # Reading the prompt can take minutes on the CPU: report progress in
        # small steps so OpenBark knows the worker is alive (it stops a
        # worker that says nothing for 120 s).
        def progress(done, total):
            if _CANCEL.is_set():
                raise _Cancelled()
            send({"id": req_id, "event": "progress", "prompt_done": int(done), "prompt_total": int(total)})

        for r in stream_generate(loaded.model, tw, mx.array(prompt), max_tokens=max_tokens,
                                 sampler=sampler, logits_processors=processors or None,
                                 prefill_step_size=512 if _on_gpu() else 64,
                                 prompt_progress_callback=progress):
            if first_ms is None:
                first_ms = int((time.time() - started) * 1000)
            last = r
            if r.text:
                parts.append(r.text)
                send({"id": req_id, "event": "token", "text": r.text})
            if r.finish_reason is not None:
                finish = r.finish_reason
            if _CANCEL.is_set():
                finish = "cancelled"
                break
    except _Cancelled:
        finish = "cancelled"
    except WorkerError:
        raise
    except MemoryError:
        raise WorkerError("inference", "ran out of memory while writing")
    except Exception as e:
        raise WorkerError("inference", "generation failed (%s)" % type(e).__name__)
    total_ms = int((time.time() - started) * 1000)
    if finish not in ("stop", "length", "cancelled"):
        finish = "stop"
    completion = int(last.generation_tokens) if last is not None else 0
    gen_secs = max(total_ms - (first_ms or 0), 1) / 1000.0
    stats = {
        "prompt_tokens": len(prompt),
        "completion_tokens": completion,
        "time_to_first_token_ms": first_ms or 0,
        "total_ms": total_ms,
        "tokens_per_second": round(float(last.generation_tps), 3) if last is not None else 0.0,
        "hit_max_tokens": finish == "length",
        "peak_memory_bytes": int(float(last.peak_memory) * 1e9) if last is not None else 0,
    }
    if last is None or not math.isfinite(stats["tokens_per_second"]):
        stats["tokens_per_second"] = round(completion / gen_secs, 3)
    log("generated prompt=%d completion=%d finish=%s ttft=%d ms total=%d ms" % (
        len(prompt), completion, finish, first_ms or 0, total_ms))
    send({"id": req_id, "event": "done", "text": "".join(parts), "finish": finish, "stats": stats})


# --------------------------------------------------------------- self-test

def _guard_holds():
    """Every path the guard covers raises NetworkBlocked (and nothing else)."""
    stream, dgram = _socket.SOCK_STREAM, _socket.SOCK_DGRAM
    probes = (
        (None, lambda s: _socket.create_connection(("127.0.0.1", 9), timeout=1)),
        (None, lambda s: _socket.getaddrinfo("huggingface.co", 443)),
        (stream, lambda s: s.connect(("127.0.0.1", 9))),
        (stream, lambda s: s.connect_ex(("127.0.0.1", 9))),
        (dgram, lambda s: s.sendto(b"x", ("127.0.0.1", 9))),
        (dgram, lambda s: s.sendmsg([b"x"], [], 0, ("127.0.0.1", 9))),
        (stream, lambda s: s.bind(("127.0.0.1", 0))),
        (stream, lambda s: s.listen(1)),
    )
    for kind, attempt in probes:
        sock = None
        if kind is not None:
            try:
                sock = _socket.socket(_socket.AF_INET, kind)
            except OSError:
                continue  # the OS itself refuses the socket (a sandbox): blocked anyway
        try:
            attempt(sock)
            return False
        except NetworkBlocked:
            pass
        except Exception:
            return False
        finally:
            if sock is not None:
                sock.close()
    return True


def op_selftest(req):
    checks = []
    message = None

    def check(name, ok, why):
        nonlocal message
        checks.append([name, bool(ok)])
        if not ok and message is None:
            message = why

    mx = None
    try:
        mx = _mlx()
        import llguidance  # noqa: F401
        import llguidance.hf  # noqa: F401
        import llguidance.numpy  # noqa: F401
        import mlx_lm  # noqa: F401
        import transformers  # noqa: F401
        from mlx_lm import stream_generate  # noqa: F401

        check("imports", True, None)
    except Exception as e:
        check("imports", False, "the MLX engine's packages didn't load (%s)" % type(e).__name__)

    check("socket_guard", _guard_holds(), "the worker's network guard isn't working")

    metal_needed = sys.platform == "darwin" and os.environ.get("OPENBARK_MLX_DEVICE", "").strip().lower() != "cpu"
    check("metal", (not metal_needed) or _DEVICE["metal"], "Metal isn't available on this Mac")

    if mx is not None:
        try:
            a = mx.random.normal((64, 64), key=mx.random.key(7))
            b = a @ mx.eye(64)
            mx.eval(b)
            check("matmul", bool(mx.allclose(a, b, atol=1e-4).item()), "MLX gave a wrong result for a matrix multiply")
        except Exception as e:
            check("matmul", False, "MLX couldn't run a matrix multiply (%s)" % type(e).__name__)
        try:
            import numpy as np

            vocab = 77  # not a multiple of 32, and wider than the mask
            bits = np.zeros((1, 2), dtype=np.uint32)
            allowed = [0, 5, 31, 32, 40, 63]
            for t in allowed:
                bits[0, t // 32] |= np.uint32(1 << (t % 32))
            words = bits.view(np.int32)
            logits = mx.random.normal((1, vocab), key=mx.random.key(11))
            out = apply_mask(mx, logits, words)
            mx.eval(out)
            got = np.array(out.astype(mx.float32))[0]
            src = np.array(logits.astype(mx.float32))[0]
            ok = all((got[i] == src[i]) if i in allowed else (got[i] == -np.inf) for i in range(vocab))
            check("mask", ok, "the note-format mask gave a wrong result")
        except Exception as e:
            check("mask", False, "the note-format mask couldn't run (%s)" % type(e).__name__)
    else:
        check("matmul", False, "MLX didn't load")
        check("mask", False, "MLX didn't load")

    ok = all(v for _, v in checks)
    log("selftest ok=%s checks=%s device=%s" % (ok, ",".join("%s:%s" % (n, v) for n, v in checks), _DEVICE["name"]))
    send({"id": req.get("id"), "event": "selftest", "ok": ok, "checks": checks, "message": message})


def op_hello(req):
    try:
        _mlx()
    except Exception as e:
        log("mlx didn't import (%s)" % type(e).__name__)
    if req.get("protocol") != PROTOCOL:
        log("protocol %r asked, %d spoken" % (req.get("protocol"), PROTOCOL))
    send({
        "event": "hello",
        "protocol": PROTOCOL,
        "python": "%d.%d.%d" % sys.version_info[:3],
        "versions": _versions(),
        "device": _DEVICE["name"],
        "metal": _DEVICE["metal"],
    })


# --------------------------------------------------------------- main loop

def main():
    threading.Thread(target=_reader, name="stdin", daemon=True).start()
    while True:
        req = _REQUESTS.get()
        op = req.get("op")
        req_id = req.get("id")
        if op == "shutdown":
            break
        _CANCEL.clear()
        try:
            if op == "hello":
                op_hello(req)
            elif op == "selftest":
                op_selftest(req)
            elif op == "load":
                op_load(req)
            elif op == "generate":
                with _STATE_LOCK:
                    _RUNNING["id"] = req_id
                try:
                    op_generate(req)
                finally:
                    with _STATE_LOCK:
                        _RUNNING["id"] = None
            elif op == "unload":
                op_unload_quietly()
                send({"id": req_id, "event": "unloaded"})
            else:
                raise WorkerError("bad_request", "unknown request")
        except WorkerError as e:
            log("error kind=%s op=%s" % (e.kind, op if op in ("hello", "selftest", "load", "generate", "unload") else "?"))
            send_error(req_id, e)
        except Exception as e:
            log("error kind=inference op=%s (%s)" % (op if isinstance(op, str) else "?", type(e).__name__))
            send_error(req_id, WorkerError("inference", "the MLX engine failed (%s)" % type(e).__name__))
    try:
        _PROTO.flush()
    except Exception:
        pass


if __name__ == "__main__":
    main()
    # Daemon threads and native libraries can delay a normal exit.
    os._exit(0)
