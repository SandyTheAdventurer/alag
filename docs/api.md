# Python API

```python
from alag import ALAG
```

## `ALAG(group, experiment, config=None, root="logs", tags=None, note=None, system_metrics=False)`

Starts logging one run to `<root>/<group>/<experiment>/`.

| Argument | Meaning |
|---|---|
| `group` | Folder that groups related runs. Flags, sections and pins are shared by a group. |
| `experiment` | Name of this run. If it's taken, `-2`, `-3`… is added. |
| `config` | Hyperparameters to save, as a dictionary. |
| `root` | Folder that holds every group. |
| `tags` | List of tags to add. |
| `note` | Note to set. |
| `system_metrics` | Log CPU, RAM and GPU use every 10 seconds. Needs `pip install 'alag[system]'`. |

`group` and `experiment` can't be empty, `.` or `..`, or contain slashes.

Attributes: `logger.path` is the run's folder, and `logger.experiment` is the name actually used.

## Methods

| Method | What it does |
|---|---|
| `log(metrics, step=None)` | Queue a dictionary of scalars. Without `step`, counts up from the last step. |
| `log_image(name, image, step=None)` | Save an image from a file path or a matplotlib figure. Without `step`, uses the last logged step. |
| `tag(*tags)` | Add tags. |
| `note(text)` | Set the note, replacing any earlier one. |
| `flag(metric, by_step, below=None, above=None)` | Set a target for the group. Pass exactly one of `below` or `above`. |
| `section(name, *metrics)` | Show these metrics' charts under one heading, for the group. |
| `pin(*names)` | Keep these charts or images at the top, for the group. |
| `flush()` | Write everything queued so far to disk. |
| `close()` | Flush and stop. Runs automatically at exit. |

`ALAG` is also a context manager: a `with` block calls `close()` at the end.

## `alag` command

```
Usage: alag [LOGDIR] [--port PORT]
       alag [LOGDIR] --export FILE

  LOGDIR         Folder of logged experiments (default: logs)
  --port PORT    Port to use (default: 6100, or the next free one)
  --export FILE  Write everything in LOGDIR to one read-only HTML file
  --version      Print the version
  --help         Print this help
```
