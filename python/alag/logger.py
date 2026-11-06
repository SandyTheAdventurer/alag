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
import threading
import time
import weakref

_FLUSH_INTERVAL = 1.0  # seconds between background writes


def _check_segment(kind, value):
    if not value or value in (".", "..") or "/" in value or "\\" in value:
        raise ValueError(f"invalid {kind} name: {value!r}")


def _check_image_name(name):
    parts = name.split("/")
    if "\\" in name or any(p in ("", ".", "..") for p in parts):
        raise ValueError(f"invalid image name: {name!r}")


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

    def __init__(self, group, experiment, config=None, root="logs"):
        _check_segment("group", group)
        _check_segment("experiment", experiment)

        self.path = _new_experiment_dir(os.path.join(root, group), experiment)
        self.experiment = os.path.basename(self.path)
        self.group = group

        with open(os.path.join(self.path, "config.json"), "w") as f:
            json.dump(config or {}, f, indent=2, default=str)

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
