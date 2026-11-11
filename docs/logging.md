# Logging

Create one `ALAG` per run. Everything it writes goes to `logs/<group>/<experiment>/`.

```python
from alag import ALAG

logger = ALAG(group="my-group", experiment="my-experiment", config={"lr": 3e-4})
```

If an experiment with that name already exists in the group, ALAG creates a new one with a number on the end, such as `my-experiment-2`, so a re-run never mixes with earlier results. `logger.experiment` holds the name it used.

## Scalars

```python
logger.log({"train/loss": loss, "train/acc": acc}, step=step)
```

Values can be Python numbers or 0-d tensors, including GPU tensors. Leave out `step` and ALAG counts up from the last step you used.

Values that aren't numbers are skipped. `nan` and `inf` are kept, and show as gaps in the chart.

The part of a name before the first slash picks the chart's section in the viewer, so `train/loss` goes under `train`.

## Images

```python
logger.log_image("confusion-matrix", "confusion.png", step=step)  # from a file
logger.log_image("loss-curve", fig, step=step)                    # from a matplotlib figure
```

The viewer shows each image name with a slider to move between steps. Names can contain slashes, such as `val/samples`.

## Hyperparameters

The `config` you pass to `ALAG` is saved as `config.json` and shown in the viewer's **Hyperparameters** tab. Nested dictionaries are fine. Values that JSON can't store are saved as text.

## Tags and notes

```python
logger.tag("baseline", "ablation")
logger.note("Loss spikes after the LR warmup ends")
```

You can also pass `tags=[...]` and `note="..."` to `ALAG`, or edit both later in the viewer.

## Flags

A flag sets a target for the whole group: a metric should cross a value by a step.

```python
logger.flag("val/loss", below=0.5, by_step=1000)
logger.flag("val/acc", above=0.9, by_step=2000)
```

The viewer draws the target on the chart and says which experiments reached it, missed it, or haven't got there yet.

## Sections and pins

```python
logger.section("validation", "val/loss", "bleu")   # show these under one heading
logger.pin("val/loss")                              # keep this at the top
```

Both apply to the whole group, so they stay in place when you compare its experiments. You can also change them in the viewer.

## System metrics

These are off by default. To turn them on, install the extra and pass `system_metrics=True`:

```sh
pip install 'alag[system]'
```

```python
logger = ALAG(group="my-group", experiment="my-experiment", system_metrics=True)
```

Every 10 seconds, a background thread logs CPU use, system RAM and your process's RAM under `system/`. If `nvidia-smi` is available, it also logs utilization, memory, temperature and power for each GPU. GPU memory is the total in use on that card, including other programs.

Each sample uses your latest training step. Before your first `logger.log`, or during a long step, several samples share a step, so set **X axis** to **Time** to read these charts.

## Speed

`logger.log` is built to stay out of your training loop. It only puts the values on an in-memory queue and returns. A background thread formats them and writes to disk about once a second.

You can pass GPU tensors straight to `logger.log`. The background thread reads them, so your training loop doesn't wait for the GPU the way it does with `loss.item()`.

On an RTX 3050 laptop GPU, a `logger.log` call with a GPU tensor took about 15 µs. A 6 ms training step was no slower with logging than without it, with or without system metrics. Any difference was smaller than the variation between runs.

`logger.log_image` does its work immediately, because matplotlib isn't thread-safe. Log images occasionally, not on every step.

## Finishing a run

Queued values are written when your program exits, including after an exception. If the process is killed outright, you can lose about the last second of values.

To finish earlier, call `logger.close()`, or use a `with` block:

```python
with ALAG(group="my-group", experiment="my-experiment") as logger:
    ...
```
