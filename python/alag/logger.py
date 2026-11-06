"""The ALAG logger.

``log()`` runs inside the training loop, so it does no I/O, no formatting and
takes no lock. It puts raw values on a deque and returns. A daemon thread does
the formatting and the disk writes.
"""

import atexit
import collections
import csv
import json
import os
import shutil
import subprocess
import threading
import time
import weakref

_FLUSH_INTERVAL = 1.0  # seconds between background writes
_SYSTEM_INTERVAL = 10.0  # seconds between system metric samples
_GPU_FIELDS = (
    ("utilization.gpu", "util_percent"),
    ("memory.used", "memory_gb"),
    ("temperature.gpu", "temperature_c"),
    ("power.draw", "power_w"),
)


def _check_segment(kind, value):
    if not value or value in (".", "..") or "/" in value or "\\" in value:
        raise ValueError(f"invalid {kind} name: {value!r}")


def _check_image_name(name):
    parts = name.split("/")
    if "\\" in name or any(p in ("", ".", "..") for p in parts):
        raise ValueError(f"invalid image name: {name!r}")


def _update_json(path, default, change):
    """Rewrite a small JSON file atomically, so the viewer never reads half of it."""
    try:
        with open(path) as f:
            data = json.load(f)
    except (FileNotFoundError, ValueError):
        data = default
    data = change(data)
    tmp = f"{path}.{os.getpid()}.tmp"
    with open(tmp, "w") as f:
        json.dump(data, f, indent=2)
    os.replace(tmp, path)


def _parse_gpus(text):
    """Turn nvidia-smi CSV output into metrics, skipping fields shown as [N/A]."""
    stats = {}
    for index, line in enumerate(text.strip().splitlines()):
        for (_, name), value in zip(_GPU_FIELDS, line.split(",")):
            try:
                number = float(value)
            except ValueError:
                continue
            stats[f"system/gpu{index}_{name}"] = (
                number / 1024 if name == "memory_gb" else number
            )
    return stats


def _read_gpus():
    """Return GPU metrics, or None if nvidia-smi isn't usable on this machine."""
    query = ",".join(field for field, _ in _GPU_FIELDS)
    try:
        result = subprocess.run(
            ["nvidia-smi", f"--query-gpu={query}", "--format=csv,noheader,nounits"],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return _parse_gpus(result.stdout) if result.returncode == 0 else None


def _new_experiment_dir(parent, experiment):
    """Create a fresh folder, adding -2, -3... if the name is already used."""
    os.makedirs(parent, exist_ok=True)
    name, n = experiment, 1
    while True:
        path = os.path.join(parent, name)
        try:
            os.mkdir(path)
            return path
        except FileExistsError:
            n += 1
            name = f"{experiment}-{n}"


class ALAG:
    """Logs one experiment to ``<root>/<group>/<experiment>/``.

    If that experiment already exists, a new one is created with a numeric
    suffix, so re-running a script never mixes two runs in the same charts.
    """

    def __init__(
        self,
        group,
        experiment,
        config=None,
        root="logs",
        tags=None,
        note=None,
        system_metrics=False,
    ):
        _check_segment("group", group)
        _check_segment("experiment", experiment)

        psutil = None
        if system_metrics:  # checked first, so a failure leaves no empty folder
            try:
                import psutil
            except ImportError:
                raise ImportError(
                    "system_metrics=True needs psutil: pip install 'alag[system]'"
                ) from None

        self.path = _new_experiment_dir(os.path.join(root, group), experiment)
        self.experiment = os.path.basename(self.path)
        self.group = group
        self._group_path = os.path.join(root, group)

        with open(os.path.join(self.path, "config.json"), "w") as f:
            json.dump(config or {}, f, indent=2, default=str)

        if tags:
            self.tag(*tags)
        if note is not None:
            self.note(note)

        self._file = open(
            os.path.join(self.path, "metrics.csv"), "w", newline="", buffering=1 << 16
        )
        self._writer = csv.writer(self._file, lineterminator="\n")
        self._writer.writerow(("step", "time", "metric", "value"))
        self._file.flush()

        self._queue = collections.deque()
        self._auto_step = 0
        self._stop = threading.Event()
        self._write_lock = threading.Lock()
        self._closed = False

        self._thread = threading.Thread(
            target=self._run, name="alag-writer", daemon=True
        )
        self._thread.start()

        self._system_thread = None
        if psutil is not None:
            self._system_thread = threading.Thread(
                target=self._watch_system, args=(psutil,), name="alag-system", daemon=True
            )
            self._system_thread.start()

        # A weak reference, so atexit doesn't keep every logger alive.
        ref = weakref.ref(self)
        self._atexit = lambda: (lambda s: s and s.close())(ref())
        atexit.register(self._atexit)

    def log(self, metrics, step=None):
        """Queue scalars. Values may be numbers or 0-d tensors.

        Tensors are detached here, not read: reading a GPU tensor would make
        the training loop wait for the GPU. The writer thread reads them.
        """
        if step is None:
            step = self._auto_step
        self._auto_step = step + 1

        items = list(metrics.items())
        for i, (key, value) in enumerate(items):
            kind = type(value)
            if kind is not float and kind is not int:
                detach = getattr(value, "detach", None)
                if detach is not None:
                    items[i] = (key, detach())
        self._queue.append((step, time.time(), items))

    def log_image(self, name, image, step=None):
        """Save an image from a file path or a matplotlib figure.

        This one runs on the calling thread: matplotlib isn't thread-safe, and
        a path may be overwritten as soon as this returns.
        """
        _check_image_name(name)
        if step is None:
            step = max(self._auto_step - 1, 0)

        folder = os.path.join(self.path, "images", *name.split("/"))
        os.makedirs(folder, exist_ok=True)

        if isinstance(image, (str, os.PathLike)):
            ext = os.path.splitext(os.fspath(image))[1].lower() or ".png"
            final = os.path.join(folder, f"{step}{ext}")
            tmp = final + ".tmp"
            shutil.copyfile(image, tmp)
        elif hasattr(image, "savefig"):
            final = os.path.join(folder, f"{step}.png")
            tmp = final + ".tmp"
            image.savefig(tmp, format="png", bbox_inches="tight")
        else:
            raise TypeError("image must be a file path or a matplotlib figure")
        os.replace(tmp, final)  # the viewer never sees a half-written image

    def tag(self, *tags):
        """Add tags to this experiment. They show up in the viewer's left panel."""

        def change(meta):
            current = meta.get("tags", [])
            meta["tags"] = current + [t for t in tags if t not in current]
            return meta

        _update_json(os.path.join(self.path, "meta.json"), {}, change)

    def note(self, text):
        """Set this experiment's note, replacing any earlier one."""

        def change(meta):
            meta["note"] = text
            return meta

        _update_json(os.path.join(self.path, "meta.json"), {}, change)

    def flag(self, metric, by_step, below=None, above=None):
        """Set a target for this group: ``metric`` should cross a value by a step.

        ``logger.flag("val/loss", below=0.5, by_step=1000)`` marks that point on
        the chart, and the viewer reports which experiments reached it in time.
        """
        if (below is None) == (above is None):
            raise ValueError("pass exactly one of below= or above=")
        flag = {
            "metric": metric,
            "direction": "below" if above is None else "above",
            "value": below if above is None else above,
            "step": by_step,
        }

        def change(flags):
            return flags if flag in flags else flags + [flag]

        _update_json(os.path.join(self._group_path, "flags.json"), [], change)

    def section(self, name, *metrics):
        """Show these metrics' charts under a heading, for the whole group.

        ``logger.section("validation", "val/loss", "bleu")``. Metrics without a
        section go under the part of their name before the first slash.
        """

        def change(layout):
            layout.setdefault("sections", {}).update(dict.fromkeys(metrics, name))
            return layout

        _update_json(os.path.join(self._group_path, "layout.json"), {}, change)

    def pin(self, *names):
        """Keep these charts or images at the top of the page, for the whole group."""

        def change(layout):
            pinned = layout.get("pinned", [])
            layout["pinned"] = pinned + [n for n in names if n not in pinned]
            return layout

        _update_json(os.path.join(self._group_path, "layout.json"), {}, change)

    def flush(self):
        """Write everything queued so far to disk."""
        with self._write_lock:
            self._drain()

    def close(self):
        """Flush and stop. Runs automatically when the program exits."""
        if self._closed:
            return
        self._closed = True
        self._stop.set()
        self._thread.join()
        if self._system_thread is not None:
            self._system_thread.join()
        self.flush()
        self._file.close()
        atexit.unregister(self._atexit)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def _run(self):
        while not self._stop.wait(_FLUSH_INTERVAL):
            self.flush()

    def _watch_system(self, psutil):
        """Sample on this thread, so nothing here touches the training loop."""
        process = psutil.Process()
        psutil.cpu_percent()  # the first reading is meaningless, it starts the count
        gpus = True
        while not self._stop.wait(_SYSTEM_INTERVAL):
            items = [
                ("system/cpu_percent", psutil.cpu_percent()),
                ("system/ram_used_gb", psutil.virtual_memory().used / 2**30),
                ("system/process_ram_gb", process.memory_info().rss / 2**30),
            ]
            if gpus:
                stats = _read_gpus()
                gpus = stats is not None
                items.extend((stats or {}).items())
            step = max(self._auto_step - 1, 0)
            self._queue.append((step, time.time(), items))

    def _drain(self):
        queue, writer = self._queue, self._writer
        wrote = False
        while queue:
            step, when, items = queue.popleft()
            for key, value in items:
                try:
                    value = float(value)  # blocks here for a GPU tensor
                except (TypeError, ValueError):
                    continue  # not a scalar, so there's nothing to chart
                writer.writerow((step, f"{when:.3f}", key, value))
                wrote = True
        if wrote:
            self._file.flush()
